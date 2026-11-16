//! Tests of what the stylesheets of CSS frameworks (Bootstrap, Bulma, Pico, Tailwind, the
//! stylesheets of Markdown renderers) rely on.

use crate::{ComputedStyle, StyledDocument, USER_AGENT_CSS, compute_styles};
use fagus_css::{MediaEnvironment, Origin, RuleSet, Stylesheet};
use fagus_dom::{Document, NodeId};

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
fn shadows_of_none_remove_earlier_shadows() {
    let (document, styled) = styles(
        "<p id=a></p><p id=b style='box-shadow: none'></p><p id=c class=v></p><p id=d class=t></p>",
        "p { box-shadow: 0 0 0 4px red; text-shadow: 1px 1px blue }
         #a { box-shadow: none; text-shadow: none }
         .v { --shadow: none; box-shadow: var(--shadow) }
         .t { -webkit-box-shadow: none; box-shadow: none }",
    );
    for id in ["a", "b", "c", "d"] {
        assert!(by_id(&document, &styled, id).box_shadow.is_empty(), "{id}");
    }
    assert!(by_id(&document, &styled, "a").text_shadow.is_empty());
    assert_eq!(by_id(&document, &styled, "b").text_shadow.len(), 1);
}

#[test]
fn line_height_numbers_are_percentages_of_the_font_size() {
    // Tailwind Typography's `prose-lg` headings: `font-size: 1.6666667em` of 18 pixels with
    // `line-height: 1.3333333` is a little under 40 pixels in Chrome, cut to 2559/64; numbers with
    // fewer digits come out whole.
    let (document, styled) = styles(
        "<div id=a></div><div id=b></div><div id=c></div><div id=d></div>",
        "#a { font-size: 30px; line-height: 1.3333333 } #b { font-size: 20px; line-height: 1.15 }
         #c { font-size: 18px; line-height: 1.7777778 } #d { font-size: 14px; line-height: 1.42857 }",
    );
    let height = |id: &str| by_id(&document, &styled, id).line_height_px(0.0) * 64.0;
    assert_eq!(height("a"), 2559.0);
    assert_eq!(height("b"), 23.0 * 64.0);
    assert_eq!(height("c"), 32.0 * 64.0);
    assert_eq!(height("d"), 1279.0);
}

#[test]
fn overflow_of_the_root_or_body_goes_to_the_viewport() {
    use crate::values::Overflow;
    let overflow = |html: &str, css: &str, id: &str| {
        let (document, styled) = styles(html, css);
        let style = by_id(&document, &styled, id);
        (style.overflow_x, style.overflow_y)
    };
    let visible = (Overflow::Visible, Overflow::Visible);
    // Bulma: `html { overflow-x: hidden }` clips nothing in print.
    let page = "<html id=h><body id=b><div id=d></div></body></html>";
    assert_eq!(overflow(page, "html { overflow-x: hidden }", "h"), visible);
    // The body's goes only where the root's is visible.
    assert_eq!(overflow(page, "body { overflow: hidden }", "b"), visible);
    assert_eq!(
        overflow(page, "html, body { overflow: hidden }", "b"),
        (Overflow::Hidden, Overflow::Hidden)
    );
    // Other elements keep theirs.
    assert_eq!(
        overflow(page, "div { overflow: hidden }", "d"),
        (Overflow::Hidden, Overflow::Hidden)
    );
}

#[test]
fn negative_paddings_from_variables_are_invalid() {
    use crate::values::LengthPercentage;
    // Open Props' `--size-00` is `-.5rem`: `padding: var(--size-00) var(--size-2)` is invalid
    // at computed-value time, so no padding applies, not even the earlier one (Chrome).
    let (document, styled) = styles(
        "<span id=a></span><span id=b></span>",
        ":root { --size-00: -.5rem; --size-2: .5rem; --gap: -4px }
         span { padding: 2px } #a { padding: var(--size-00) var(--size-2) }
         #b { padding-left: calc(var(--gap) * -1) }",
    );
    let left = |id: &str| by_id(&document, &styled, id).padding.left.clone();
    assert_eq!(left("a"), LengthPercentage::ZERO);
    // Inside a function a negative number is fine.
    assert_eq!(left("b"), LengthPercentage::Px(4.0));
}

//! Tests of what documents exported by office programs and generators (Microsoft Word,
//! `LibreOffice`, Google Docs, e-mail frameworks) rely on.

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
fn page_names_keep_their_case() {
    // Word names its sections' pages `WordSection1`; a page name is a case-sensitive identifier.
    let (document, styled) = styles(
        "<div id=a class=WordSection1></div><div id=b></div><div id=c></div>",
        "div.WordSection1 { page: WordSection1 } #b { page: AUTO } #c { page: Cover }",
    );
    assert_eq!(
        by_id(&document, &styled, "a").page.as_deref(),
        Some("WordSection1")
    );
    assert_eq!(by_id(&document, &styled, "b").page, None);
    assert_eq!(by_id(&document, &styled, "c").page.as_deref(), Some("Cover"));
}

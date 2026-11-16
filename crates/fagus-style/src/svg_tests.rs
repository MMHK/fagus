//! The styles of SVG elements in HTML (their transforms, clip paths and the presentation
//! attributes Fagus' cascade reads), and the newer basic shapes, as Chrome 157 computes them.

use fagus_css::{MediaEnvironment, Origin, RuleSet, Stylesheet};
use fagus_dom::{Document, NodeId};

use crate::values::{ClipShape, LengthPercentage, TransformBox, TransformFunction};
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
fn reads_the_transform_attributes_of_svg_elements() {
    let (document, styled) = styles(
        r#"<svg id="outer" transform-origin="1 2"><rect id="attribute" transform="rotate(90 10 20)" transform-origin="75"/>
        <rect id="css" transform="scale(2)" class="turned"/><rect id="invalid" transform="rotate(90,)"/>
        <g id="plain"/></svg>"#,
        ".turned { transform: rotate(45deg); transform-box: fill-box; vector-effect: non-scaling-stroke }",
    );
    let style = |id| by_id(&document, &styled, id);
    // The attribute is a transform in pixels and degrees; `transform-origin` takes numbers as
    // pixels and the center for a missing value.
    assert_eq!(
        &*style("attribute").transform,
        [
            TransformFunction::Translate(LengthPercentage::Px(10.0), LengthPercentage::Px(20.0)),
            TransformFunction::Rotate(90.0),
            TransformFunction::Translate(LengthPercentage::Px(-10.0), LengthPercentage::Px(-20.0)),
        ]
    );
    assert_eq!(
        style("attribute").transform_origin,
        (LengthPercentage::Px(75.0), LengthPercentage::Percent(0.5))
    );
    // A style sheet wins over the attribute.
    assert_eq!(&*style("css").transform, [TransformFunction::Rotate(45.0)]);
    assert_eq!(style("css").transform_box, TransformBox::FillBox);
    assert!(
        style("css")
            .svg_paint
            .iter()
            .any(|(name, value)| *name == "vector-effect" && value == "non-scaling-stroke")
    );
    // An attribute with an error is no transform.
    assert!(!style("invalid").has_transform());
    // SVG elements turn about their origin; an outer `<svg>` is a CSS box, which keeps the
    // center and does not read the attribute.
    assert_eq!(
        style("plain").transform_origin,
        (LengthPercentage::Px(0.0), LengthPercentage::Px(0.0))
    );
    assert_eq!(style("plain").transform_box, TransformBox::ViewBox);
    assert_eq!(
        style("outer").transform_origin,
        (LengthPercentage::Percent(0.5), LengthPercentage::Percent(0.5))
    );
}

#[test]
fn reads_basic_shapes_of_clip_path_attributes() {
    let (document, styled) = styles(
        r#"<svg><rect id="circle" clip-path="circle(50%) fill-box"/><rect id="url" clip-path="url(#c)"/>
        <rect id="view" clip-path="inset(1px) view-box"/><rect id="default" clip-path="inset(1px)"/>
        <rect id="bad" clip-path="inset(1px);fill:red"/></svg>
        <div id="html" style="clip-path: circle(1px) fill-box; backdrop-filter: blur(2px)"></div>"#,
        "",
    );
    let clip = |id| by_id(&document, &styled, id).clip_path.clone();
    let circle = clip("circle").expect("circle");
    assert!(matches!(circle.shape, ClipShape::Ellipse { circle: true, .. }));
    assert_eq!(circle.svg_reference, TransformBox::FillBox);
    // `url()`s are left to the SVG renderer.
    assert_eq!(clip("url"), None);
    assert_eq!(clip("view").expect("view").svg_reference, TransformBox::ViewBox);
    assert_eq!(
        clip("default").expect("default").svg_reference,
        TransformBox::StrokeBox
    );
    assert_eq!(clip("bad"), None);
    // `fill-box` is the content box of a CSS box.
    let html = by_id(&document, &styled, "html");
    assert_eq!(
        html.clip_path.as_ref().expect("html").reference,
        crate::values::BoxArea::ContentBox
    );
    // `backdrop-filter` makes a containing block and a stacking context.
    assert!(html.contains_out_of_flow() && html.is_stacking_context());
}

#[test]
fn computes_positions_control_points_and_subpaths_of_shape() {
    let (document, styled) = styles(
        r#"<div id="keywords"></div><div id="relative"></div><div id="lines"></div><div id="subpaths"></div>
        <div id="units"></div>"#,
        "#keywords { clip-path: shape(from center left, curve to center right with center top / center top, close) }
         #relative { clip-path: shape(from 0 0, curve by 100px 0 with 50% 10px from start / -10px 0 from end) }
         #lines { clip-path: shape(from top left, hline to center, vline to bottom, hline to left, close) }
         #subpaths { clip-path: shape(evenodd from 0 0, hline by 10px, vline by 10px, close, move to 20px 20px, hline by 5px) }
         #units { font-size: 10px; clip-path: shape(from 1em 2em, line to calc(10px + 50%) 0) }",
    );
    let points = |id| {
        let clip = by_id(&document, &styled, id)
            .clip_path
            .clone()
            .expect("clip path");
        let ClipShape::Polygon { points, .. } = &clip.shape else {
            panic!("shape() is a polygon: {clip:?}");
        };
        points
            .iter()
            .map(|(x, y)| (x.resolve(200.0), y.resolve(100.0)))
            .collect::<Vec<_>>()
    };
    let keywords = points("keywords");
    assert_eq!(keywords[0], (0.0, 50.0));
    assert_eq!(*keywords.last().expect("end"), (200.0, 50.0));
    // Halfway along a cubic curve with both control points at the top center.
    assert_eq!(keywords[8], (100.0, 12.5));
    let relative = points("relative");
    // Control points from the start (100, 10) and from the end (90, 0) of the curve.
    assert_eq!(
        relative[8],
        (100.0 * 0.375 + 90.0 * 0.375 + 100.0 * 0.125, 10.0 * 0.375)
    );
    assert_eq!(
        points("lines"),
        [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)]
    );
    // The second subpath starts from the first's start and comes back to it.
    assert_eq!(
        points("subpaths"),
        [
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 0.0),
            (20.0, 20.0),
            (25.0, 20.0),
            (20.0, 20.0)
        ]
    );
    assert_eq!(points("units"), [(10.0, 20.0), (110.0, 0.0)]);
}

#[test]
fn reads_the_overflow_attribute_and_the_mask_type_of_svg_elements() {
    use crate::values::Overflow;
    let (document, styled) = styles(
        r#"<svg id="shown" overflow="visible"></svg><svg id="auto" overflow="auto"></svg>
        <svg id="css" overflow="visible"></svg><svg id="plain"><mask id="m"/></svg>"#,
        "#css { overflow: hidden } #m { mask-type: alpha }",
    );
    let overflow = |id| {
        let style = by_id(&document, &styled, id);
        (style.overflow_x, style.overflow_y)
    };
    assert_eq!(overflow("shown"), (Overflow::Visible, Overflow::Visible));
    assert_eq!(overflow("auto"), (Overflow::Visible, Overflow::Visible));
    // A style sheet wins over the attribute; without either, svg elements clip (`svg.css`).
    assert_eq!(overflow("css"), (Overflow::Hidden, Overflow::Hidden));
    assert_eq!(overflow("plain"), (Overflow::Clip, Overflow::Clip));
    // `mask-type` goes to the SVG renderer.
    assert!(
        by_id(&document, &styled, "m")
            .svg_paint
            .iter()
            .any(|(name, value)| *name == "mask-type" && value == "alpha")
    );
}

#[test]
fn reads_filter_attributes_with_functions_or_other_documents() {
    use crate::values::Filter;
    let (document, styled) = styles(
        r#"<svg><rect id="shadow" filter="drop-shadow(5px 0 green)"/><rect id="own" filter="url(#f)"/>
        <rect id="other" filter="url(data:image/svg+xml;base64,PHN2Zy8+#f)"/><rect id="bad" filter="blur(1px);color:red"/></svg>"#,
        "",
    );
    let filter = |id| by_id(&document, &styled, id).filter.clone();
    assert!(matches!(&*filter("shadow"), [Filter::DropShadow { x, .. }] if (*x - 5.0).abs() < 1e-6));
    // The SVG renderer finds the filters of its own document itself.
    assert!(filter("own").is_empty());
    assert!(matches!(&*filter("other"), [Filter::Url(url)] if url.starts_with("data:")));
    // Declarations do not come in through the attribute.
    assert!(filter("bad").is_empty());
}

#[test]
fn takes_the_boxes_of_svg_elements_for_mask_origins_and_clips() {
    let (document, styled) = styles(
        r#"<div id="html"></div><svg><rect id="svg"/></svg>"#,
        "#html, #svg { mask-image: linear-gradient(black, black), linear-gradient(red, red);
         mask-origin: content-box, view-box; mask-clip: no-clip, padding-box }",
    );
    let mask = |id| by_id(&document, &styled, id).mask.clone().expect("a mask");
    let svg = mask("svg");
    assert_eq!(&*svg.svg_origins, [TransformBox::FillBox, TransformBox::ViewBox]);
    assert_eq!(&*svg.svg_clips, [None, Some(TransformBox::FillBox)]);
    // Boxes of CSS keep theirs.
    assert_eq!(mask("html").origins, svg.origins);
    // Without them, the stroke box.
    let (document, styled) = styles(
        r#"<svg><rect id="plain"/></svg>"#,
        "#plain { mask-image: linear-gradient(black, black) }",
    );
    let plain = by_id(&document, &styled, "plain").mask.clone().expect("a mask");
    assert_eq!(&*plain.svg_origins, [TransformBox::StrokeBox]);
    assert_eq!(&*plain.svg_clips, [Some(TransformBox::StrokeBox)]);
}

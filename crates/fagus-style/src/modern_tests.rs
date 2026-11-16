//! Modern CSS that Chrome prints: color schemes, cascade keywords, queries and functions that are
//! resolved for each element.

use fagus_css::{MediaEnvironment, Origin, RuleSet, Stylesheet};
use fagus_dom::{Document, NodeId};

use crate::values::Rgba;
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

fn rgb(r: u8, g: u8, b: u8) -> Rgba {
    Rgba::from_u8(r, g, b, 1.0)
}

#[test]
fn follows_the_color_scheme_in_light_dark_and_system_colors() {
    let (document, styled) = styles(
        r#"<body><p id="text">x</p><div id="dark" style="background: light-dark(red, blue)"></div>
        <div id="light" style="color-scheme: light; background: light-dark(red, blue)"></div>
        <div id="both" style="color-scheme: dark light; background: light-dark(red, blue)"></div>
        <div id="var" style="color-scheme: light; background: var(--c)"></div>
        <div id="canvas" style="background: Canvas; color: CanvasText"></div>
        <input id="field"></body>"#,
        ":root { color-scheme: dark; --c: light-dark(red, blue) }",
    );
    let style = |id| by_id(&document, &styled, id);
    assert_eq!(style("text").color, Rgba::WHITE);
    assert_eq!(style("dark").background_color, rgb(0, 0, 255));
    assert_eq!(style("light").background_color, rgb(255, 0, 0));
    assert_eq!(style("both").background_color, rgb(255, 0, 0));
    assert_eq!(style("var").background_color, rgb(255, 0, 0));
    assert_eq!(style("canvas").background_color, rgb(0x12, 0x12, 0x12));
    assert_eq!(style("canvas").color, Rgba::WHITE);
    assert_eq!(style("field").background_color, rgb(0x3b, 0x3b, 0x3b));
    assert_eq!(style("field").color, Rgba::WHITE);
}

#[test]
fn rolls_back_to_the_origin_or_layer_before() {
    let (document, styled) = styles(
        r#"<div id="origin"></div><div id="layer" class="t"></div><div id="unlayered" class="u"></div>
        <div id="first" class="f"></div><p id="all" class="a"></p>"#,
        "#origin { display: inline; display: revert; color: red; color: revert }
        @layer base, top;
        @layer base { .t { color: green } .u { color: green } .f { color: blue } }
        @layer top { .t { color: revert-layer } }
        .u { color: red } .u { color: revert-layer }
        @layer base { .f { color: revert-layer } }
        .a { margin: 0; all: revert }",
    );
    let style = |id| by_id(&document, &styled, id);
    assert_eq!(style("origin").display, crate::values::Display::BLOCK);
    assert_eq!(style("origin").color, Rgba::BLACK);
    assert_eq!(style("layer").color, rgb(0, 128, 0));
    assert_eq!(style("unlayered").color, rgb(0, 128, 0));
    assert_eq!(style("first").color, Rgba::BLACK);
    // The user agent's margins of a paragraph: 1em.
    assert_eq!(
        style("all").margin.top,
        crate::values::LengthPercentageAuto::Length(crate::values::LengthPercentage::Px(16.0))
    );
}

#[test]
fn answers_container_style_queries() {
    let (document, styled) = styles(
        r#"<div class="card"><p id="parent">x</p><section><p id="named">x</p></section></div>
        <div class="plain"><p id="other">x</p></div>"#,
        ".card { --theme: dark; --n: 3; container-name: card }
        .plain { --theme: light }
        @container style(--theme: dark) { p { color: green } }
        @container card style(--n: 3) and style(--theme) { #named { color: blue } }
        @container style(not (--theme: dark)) { p { color: red } }",
    );
    let style = |id| by_id(&document, &styled, id);
    assert_eq!(style("parent").color, rgb(0, 128, 0));
    assert_eq!(style("named").color, rgb(0, 0, 255));
    assert_eq!(style("other").color, rgb(255, 0, 0));
}

fn width(style: &ComputedStyle) -> Option<f32> {
    match &style.width {
        crate::values::Size::Length(length) => length.fixed(),
        _ => None,
    }
}

#[test]
fn resolves_functions_for_each_element() {
    let (document, styled) = styles(
        r#"<ul><li id="first"></li><li></li><li id="third"></li></ul>
        <div id="typed" data-w="55px"></div><div id="unit" data-w="65"></div>
        <div id="wrong" data-w="red"></div><div id="missing"></div>
        <div id="print"></div><div id="width"></div><div id="style"></div><div id="none"></div>
        <div id="progress"></div><div id="env"></div><div id="round"></div>
        <div id="dark" style="background: contrast-color(#222)"></div>
        <div id="light" style="background: contrast-color(#eee)"></div>"#,
        "li { width: calc(sibling-index() * 10px); height: calc(sibling-count() * 1px) }
        #typed { width: attr(data-w type(<length>)) }
        #unit { width: attr(data-w px) }
        #wrong, #missing { width: attr(data-w type(<length>), 7px) }
        #print { width: if(media(print): 70px; else: 20px) }
        #width { width: if(not media(width < 100px): 70px; else: 20px) }
        #style { --x: 2; width: if(style(--x: 1): 70px; style(--x: 2): 40px; else: 20px) }
        #none { width: 5px; width: if(supports(display: nonsense): 1px) }
        #progress { width: calc(progress(90px, 0px, 60px) * 100px) }
        #env { width: calc(env(safe-area-inset-left, 33px) + env(titlebar-area-width, 15px)) }
        #round { width: calc(1px * round(2.5)) }",
    );
    let style = |id| by_id(&document, &styled, id);
    assert_eq!(width(style("first")), Some(10.0));
    assert_eq!(width(style("third")), Some(30.0));
    assert_eq!(
        style("first").height,
        crate::values::Size::Length(crate::values::LengthPercentage::Px(3.0))
    );
    assert_eq!(width(style("typed")), Some(55.0));
    assert_eq!(width(style("unit")), Some(65.0));
    assert_eq!(width(style("wrong")), Some(7.0));
    assert_eq!(width(style("missing")), Some(7.0));
    // `media()` takes conditions alone: a media type is unknown, and so false.
    assert_eq!(width(style("print")), Some(20.0));
    assert_eq!(width(style("width")), Some(70.0));
    assert_eq!(width(style("style")), Some(40.0));
    // No branch holds: invalid at computed-value time, so `auto`.
    assert_eq!(style("none").width, crate::values::Size::Auto);
    assert_eq!(width(style("progress")), Some(100.0));
    assert_eq!(width(style("env")), Some(15.0));
    assert_eq!(width(style("round")), Some(3.0));
    assert_eq!(style("dark").background_color, Rgba::WHITE);
    assert_eq!(style("light").background_color, Rgba::BLACK);
}

#[test]
fn checks_registered_custom_properties_against_their_syntax() {
    let (document, styled) = styles(
        r#"<div id="outer"><div id="bad"></div><div id="good"></div></div>"#,
        "@property --len { syntax: '<length>'; inherits: false; initial-value: 25px }
        @property --inh { syntax: '<length>'; inherits: true; initial-value: 5px }
        #outer { --inh: 45px }
        #bad { --len: red; --inh: blue; width: var(--len); height: var(--inh) }
        #good { --len: 12px; width: var(--len) }",
    );
    let style = |id| by_id(&document, &styled, id);
    assert_eq!(width(style("bad")), Some(25.0));
    assert_eq!(
        style("bad").height,
        crate::values::Size::Length(crate::values::LengthPercentage::Px(45.0))
    );
    assert_eq!(width(style("good")), Some(12.0));
}

#[test]
fn computes_newer_shapes_of_shape_outside() {
    use crate::values::{ClipShape, LengthPercentage, ShapeOutside};
    let (document, styled) = styles(
        r#"<div id="xywh"></div><div id="rect"></div><div id="shape"></div><div id="curve"></div>"#,
        "div { float: left; width: 100px; height: 100px }
        #xywh { shape-outside: xywh(10px 20px 50% 60px) }
        #rect { shape-outside: rect(10px auto 30px 0) border-box }
        #shape { shape-outside: shape(evenodd from 0 0, hline to 100%, vline by 50%, line to 0 100%, close) }
        #curve { shape-outside: shape(from 0 0, curve to 100px 0 with 50px 50px) }",
    );
    let shape = |id| match by_id(&document, &styled, id).shape_outside.as_deref() {
        Some(ShapeOutside::Shape { shape, .. }) => shape.clone(),
        other => panic!("no shape: {other:?}"),
    };
    let ClipShape::Inset { insets, .. } = shape("xywh") else {
        panic!("xywh() is an inset");
    };
    assert_eq!(insets[0], LengthPercentage::Px(20.0));
    assert_eq!(insets[1].resolve(100.0), 40.0);
    assert_eq!(insets[2].resolve(100.0), 20.0);
    assert_eq!(insets[3], LengthPercentage::Px(10.0));
    let ClipShape::Inset { insets, .. } = shape("rect") else {
        panic!("rect() is an inset");
    };
    assert_eq!(insets[1], LengthPercentage::Px(0.0));
    assert_eq!(insets[2].resolve(100.0), 70.0);
    let ClipShape::Polygon { even_odd, points } = shape("shape") else {
        panic!("shape() is a polygon");
    };
    assert!(even_odd);
    let resolved: Vec<(f32, f32)> = points
        .iter()
        .map(|(x, y)| (x.resolve(100.0), y.resolve(100.0)))
        .collect();
    assert_eq!(resolved, [(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 100.0)]);
    let ClipShape::Polygon { points, .. } = shape("curve") else {
        panic!("shape() is a polygon");
    };
    // The curve is cut into lines; its middle is halfway to its control point.
    assert_eq!(points.len(), 17);
    assert_eq!(points[8].1.resolve(100.0), 25.0);
}

#[test]
fn clamps_lines_with_the_prefixed_property_alone() {
    let (document, styled) = styles(
        r#"<div id="standard"></div><div id="prefixed"></div>"#,
        "#standard { line-clamp: 2; overflow: hidden }
        #prefixed { display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2 }",
    );
    // Chrome 157 does not take `line-clamp` yet.
    assert_eq!(by_id(&document, &styled, "standard").line_clamp, None);
    assert_eq!(by_id(&document, &styled, "prefixed").line_clamp, Some(2));
}

#[test]
fn clips_colors_outside_of_srgb_channel_by_channel() {
    let (document, styled) = styles(
        r#"<div id="oklch"></div><div id="p3"></div>"#,
        "#oklch { background: oklch(0.7 0.3 150) } #p3 { background: color(display-p3 0 1 0) }",
    );
    let oklch = by_id(&document, &styled, "oklch").background_color;
    assert_eq!((oklch.r, oklch.b), (0.0, 0.0));
    assert!((oklch.g - 0.7968).abs() < 0.001, "{oklch:?}");
    assert_eq!(by_id(&document, &styled, "p3").background_color, rgb(0, 255, 0));
}

#[test]
fn takes_corner_shapes_as_curvatures() {
    let (document, styled) = styles(
        r#"<div id="one"></div><div id="four"></div><div id="side"></div><div id="bad"></div><div id="corner"></div>"#,
        "#corner { corner-top-left-shape: scoop; corner-bottom-right-shape: notch }
        #one { corner-shape: bevel }
        #four { corner-shape: scoop squircle superellipse(3) notch }
        #side { corner-shape: bevel; corner-top-shape: square round }
        #bad { corner-shape: bevel; corner-shape: wobbly }",
    );
    assert_eq!(by_id(&document, &styled, "one").corner_shape, [1.0; 4]);
    assert_eq!(
        by_id(&document, &styled, "four").corner_shape,
        [0.5, 4.0, 8.0, 0.001]
    );
    assert_eq!(
        by_id(&document, &styled, "side").corner_shape,
        [1000.0, 2.0, 1.0, 1.0]
    );
    // An invalid value leaves the one before.
    assert_eq!(by_id(&document, &styled, "bad").corner_shape, [1.0; 4]);
    assert_eq!(
        by_id(&document, &styled, "corner").corner_shape,
        [0.5, 2.0, 0.001, 2.0]
    );
}

#[test]
fn keeps_anchor_positioning_for_layout() {
    let (document, styled) = styles(
        r#"<div id="anchor"></div><div id="tip"></div>"#,
        "#anchor { anchor-name: --a, --b; anchor-scope: all }
        #tip { position: absolute; position-anchor: --a; top: calc(anchor(bottom) + 4px);
               inset-inline-start: anchor(left); width: anchor-size(width); justify-self: anchor-center;
               position-area: bottom span-right; position-try-fallbacks: flip-block }",
    );
    let anchor = by_id(&document, &styled, "anchor")
        .anchoring
        .clone()
        .expect("anchor");
    assert_eq!(anchor.names, ["--a", "--b"]);
    assert_eq!(anchor.scope, crate::anchor::AnchorScope::All);
    let tip = by_id(&document, &styled, "tip").anchoring.clone().expect("tip");
    assert_eq!(tip.default.as_deref(), Some("--a"));
    assert!(tip.insets[0].is_some() && tip.insets[3].is_some());
    assert!(tip.sizes[0].is_some());
    assert_eq!(tip.center, [true, false]);
    assert!(tip.area.is_some());
    assert_eq!(tip.fallbacks.len(), 1);
}

#[test]
fn applies_position_try_rules_to_their_options() {
    let (document, styled) = styles(
        r#"<div id="tip" style="margin-top: 3px"></div>"#,
        "@position-try --below { position-area: bottom; width: 50px; margin-top: 8px; color: red }
        @position-try --below { position-area: bottom; width: 60px; margin-top: 9px }
        #tip { position: absolute; position-anchor: --a; position-area: top; width: 10px;
               position-try-fallbacks: --below flip-inline, --missing }",
    );
    let tip = by_id(&document, &styled, "tip");
    let anchoring = tip.anchoring.clone().expect("tip");
    let [
        crate::anchor::TryOption::Rule {
            name,
            flip,
            style: Some(tried),
        },
        crate::anchor::TryOption::Rule { style: None, .. },
    ] = anchoring.fallbacks.as_slice()
    else {
        panic!(
            "a rule that applies and one that is not there: {:?}",
            anchoring.fallbacks
        );
    };
    assert_eq!((name.as_str(), *flip), ("--below", (false, true, false)));
    // The last rule of the name, over the element's own declarations (its inline style too),
    // but only for the properties such a rule may set.
    assert_eq!(
        tried.width,
        crate::values::Size::Length(crate::values::LengthPercentage::Px(60.0))
    );
    assert_eq!(
        tried.margin.top,
        crate::values::LengthPercentageAuto::Length(crate::values::LengthPercentage::Px(9.0))
    );
    assert_eq!(tried.color, tip.color);
    let area = tried.anchoring.as_deref().and_then(|anchoring| anchoring.area);
    assert_eq!(area.map(|area| area.rows), Some((2, 2)));
    assert_eq!(
        tip.width,
        crate::values::Size::Length(crate::values::LengthPercentage::Px(10.0))
    );
}

#[test]
fn resolves_spacing_percentages_against_each_font_size() {
    // A percentage of the font size, inherited as such: each element resolves it against its
    // own font size, as Chrome does; with a length in `calc()`, that length is fixed where it is
    // declared.
    let (document, styled) = styles(
        r#"<div id="letters" style="font-size: 20px; letter-spacing: 10%">
             <span id="big" style="font-size: 40px">x</span></div>
           <div id="words" style="font-size: 20px; word-spacing: calc(0.5em + 50%)">
             <span id="small" style="font-size: 10px">x</span></div>
           <div id="reset" style="letter-spacing: 10%"><span id="fixed" style="letter-spacing: 2px; font-size: 30px">x</span></div>"#,
        "",
    );
    let style = |id| by_id(&document, &styled, id);
    assert!((style("letters").letter_spacing - 2.0).abs() < 1e-4);
    assert!((style("big").letter_spacing - 4.0).abs() < 1e-4);
    assert!((style("words").word_spacing - 20.0).abs() < 1e-4);
    assert!((style("small").word_spacing - 15.0).abs() < 1e-4);
    assert!((style("fixed").letter_spacing - 2.0).abs() < 1e-4);
}

#[test]
fn computes_hyphenate_limit_chars() {
    let (document, styled) = styles(
        r#"<div id="one" style="hyphenate-limit-chars: 8"><p id="inherited">x</p></div>
           <div id="three" style="hyphenate-limit-chars: auto 3 2"></div>
           <div id="invalid" style="hyphenate-limit-chars: 0 2"></div>"#,
        "",
    );
    let style = |id| by_id(&document, &styled, id);
    assert_eq!(style("one").hyphenate_limit_chars, [8, 0, 0]);
    assert_eq!(style("inherited").hyphenate_limit_chars, [8, 0, 0]);
    assert_eq!(style("three").hyphenate_limit_chars, [0, 3, 2]);
    assert_eq!(style("invalid").hyphenate_limit_chars, [0, 0, 0]);
}

#[test]
fn makes_boxes_with_layout_containment_or_will_change_contain_positioned_boxes() {
    let (document, styled) = styles(
        r#"<div id="layout" style="contain: layout"></div><div id="paint" style="contain: paint"></div>
        <div id="content" style="contain: content"></div><div id="size" style="contain: size"></div>
        <div id="transform" style="will-change: transform"></div>
        <div id="filter" style="will-change: opacity, filter"></div>
        <div id="opacity" style="will-change: opacity"></div><div id="auto" style="will-change: auto"></div>"#,
        "",
    );
    for id in ["layout", "paint", "content", "transform", "filter"] {
        let style = by_id(&document, &styled, id);
        assert!(style.contains_absolute() && style.is_stacking_context(), "{id}");
    }
    for id in ["size", "opacity", "auto"] {
        assert!(!by_id(&document, &styled, id).contains_out_of_flow(), "{id}");
    }
}

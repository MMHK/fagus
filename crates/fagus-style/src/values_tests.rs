//! Colors, images and values as Chrome 157 computes them: `currentcolor` in color functions and
//! inherited, `display-p3-linear`, `image(<color>)`, `light-dark()` of images, gradient stops
//! that miss components, registered colors in style queries, ratios of table parts and
//! interpolated animations.

use fagus_css::{MediaEnvironment, Origin, RuleSet, Stylesheet};
use fagus_dom::{Document, NodeId};

use crate::values::{BoxShadow, GradientStop, Image, LengthPercentage, Rgba, Size};
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

fn bytes(color: Rgba) -> [u8; 4] {
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "0 to 255")]
    [color.r, color.g, color.b, color.a].map(|channel| (channel * 255.0).round() as u8)
}

/// The color of a gradient stop, and the color the gradient goes on from there, as bytes.
type StopColors = (Option<[u8; 4]>, Option<[u8; 4]>);

/// The colors of the stops of a style's first background gradient.
fn stops(style: &ComputedStyle) -> Vec<StopColors> {
    let Some(Image::Gradient(gradient)) = style.background_image.first() else {
        panic!("no gradient: {:?}", style.background_image);
    };
    gradient
        .stops
        .iter()
        .filter_map(|stop| match stop {
            GradientStop::Color { color, after, .. } => Some((color.map(bytes), after.map(bytes))),
            GradientStop::Hint(_) => None,
        })
        .collect()
}

#[test]
fn resolves_currentcolor_in_color_functions_and_inherits_it_as_the_keyword() {
    let (document, styled) = styles(
        r#"<div id="rgb"><div id="rgb-child"></div></div><div id="mix"><div id="mix-child"></div></div>
        <div id="plain"><div id="plain-child"><div id="grandchild"></div></div></div>
        <div id="shorthand"><div id="shorthand-child"></div></div>
        <div id="border"></div><div id="own" style="color: color-mix(in srgb, currentcolor, white)"></div>"#,
        "div { color: red } div div { color: green }
        #rgb { background-color: rgb(from currentColor r g b) }
        #mix { background-color: color-mix(in srgb, currentColor 50%, blue) }
        #plain { background-color: currentcolor } #grandchild { color: blue }
        #shorthand { background: no-repeat color-mix(in srgb, currentcolor 50%, white) }
        #rgb-child, #mix-child, #plain-child, #grandchild, #shorthand-child { background: inherit }
        #border { color: blue; border-top-color: rgb(from currentcolor r g b / 50%) }",
    );
    let background = |id| bytes(by_id(&document, &styled, id).background_color);
    assert_eq!(background("rgb"), [255, 0, 0, 255]);
    assert_eq!(background("rgb-child"), [0, 128, 0, 255], "the child's own color");
    assert_eq!(background("mix"), [128, 0, 128, 255]);
    assert_eq!(background("mix-child"), [0, 64, 128, 255]);
    assert_eq!(background("plain-child"), [0, 128, 0, 255]);
    assert_eq!(background("grandchild"), [0, 0, 255, 255]);
    assert_eq!(background("shorthand-child"), [128, 192, 128, 255]);
    let border = by_id(&document, &styled, "border").border_color.top.map(bytes);
    assert_eq!(border, Some([0, 0, 255, 128]));
    // In `color`, `currentcolor` is the parent's color (the body's black).
    assert_eq!(
        bytes(by_id(&document, &styled, "own").color),
        [128, 128, 128, 255]
    );
}

#[test]
fn computes_display_p3_linear_colors() {
    let (document, styled) = styles(
        r#"<div id="green"></div><div id="white"></div><div id="relative"></div><div id="mixed"></div>"#,
        "#green { background-color: color(display-p3-linear 0.0383 0.2087 0.0156) }
        #white { background-color: color(display-p3-linear 1 1 1 / 50%) }
        #relative { background-color: color(from green display-p3-linear r g b) }
        #mixed { background-color: color-mix(in display-p3-linear, color(display-p3-linear 0 1 0), green 100%) }",
    );
    let background = |id| bytes(by_id(&document, &styled, id).background_color);
    assert_eq!(background("green"), [0, 128, 0, 255]);
    assert_eq!(background("white"), [255, 255, 255, 128]);
    assert_eq!(background("relative"), [0, 128, 0, 255]);
    assert_eq!(background("mixed"), [0, 128, 0, 255]);
}

#[test]
fn draws_image_of_a_color_and_light_dark_images() {
    let (document, styled) = styles(
        r#"<div id="color"></div><div id="current"></div><div id="url"></div>
        <div id="dark"></div><div id="light"></div>"#,
        "#color { background-image: image(green) }
        #current { color: blue; background-image: image(currentcolor) }
        #url { background-color: red; background: image('a.png', green) }
        #dark, #light { background-image: light-dark(linear-gradient(red, red), url(dark.png)) }
        #dark { color-scheme: dark } #light { color-scheme: light }",
    );
    let style = |id| by_id(&document, &styled, id);
    assert_eq!(stops(style("color")), vec![(Some([0, 128, 0, 255]), None); 2]);
    assert_eq!(stops(style("current")), vec![(Some([0, 0, 255, 255]), None); 2]);
    // Chrome takes no images in `image()`: the declaration is invalid.
    assert_eq!(bytes(style("url").background_color), [255, 0, 0, 255]);
    assert!(
        matches!(style("dark").background_image.first(), Some(Image::Url(url, _)) if &**url == "dark.png")
    );
    assert_eq!(stops(style("light")), vec![(Some([255, 0, 0, 255]), None); 2]);
}

#[test]
fn splits_gradient_stops_that_miss_components() {
    let (document, styled) = styles(
        r#"<div id="srgb"></div><div id="legacy"></div><div id="hue"></div>"#,
        "#srgb { background: linear-gradient(to right in srgb, color(srgb 0.5 0 0), color(srgb none 0.5 0.5), white) }
        #legacy { background: linear-gradient(90deg in srgb, rgb(none 255 none), yellow) }
        #hue { background: linear-gradient(in oklch, oklch(0.5 0.1 90), oklch(0.5 0.1 none), oklch(0.5 0.1 180)) }",
    );
    let style = |id| by_id(&document, &styled, id);
    // A missing red takes the red before up to the stop and the red after beyond it.
    let srgb = stops(style("srgb"));
    assert_eq!(srgb[1], (Some([128, 128, 128, 255]), Some([255, 128, 128, 255])));
    // So do the components of legacy colors, which lightningcss writes as hex.
    assert_eq!(stops(style("legacy"))[0], (Some([255, 255, 0, 255]), None));
    let hue = stops(style("hue"));
    assert_eq!(hue[1].0, hue[0].0);
    assert_eq!(hue[1].1, hue[2].0);
}

#[test]
fn compares_registered_colors_in_style_queries_as_colors() {
    let (document, styled) = styles(
        r#"<div id="root"><p id="text">x</p></div>"#,
        "@property --contrast { syntax: '<color>'; initial-value: white; inherits: true }
        @container style(--contrast: color(srgb 1 1 1)) { p { color: green } }",
    );
    assert_eq!(bytes(by_id(&document, &styled, "text").color), [0, 128, 0, 255]);
}

#[test]
fn takes_no_ratio_on_table_parts() {
    let (document, styled) = styles(
        r#"<table id="table"><tr id="row"><td id="cell">x</td><caption id="caption"></caption></tr></table>"#,
        "#table, #row, #cell, #caption { aspect-ratio: 2 }",
    );
    let ratio = |id| by_id(&document, &styled, id).aspect_ratio;
    assert_eq!(ratio("table"), Some(2.0));
    assert_eq!(ratio("row"), None);
    assert_eq!(ratio("cell"), None);
}

#[test]
fn interpolates_shadows_and_sizes_of_animations() {
    let (document, styled) = styles(
        r#"<div id="shadow"></div><div id="steps"></div><div id="size"></div><div id="mixed"></div>"#,
        "@keyframes shadow { from { box-shadow: blue 100px 0 } to { box-shadow: lime 140px 20px 10px, red 0 0 } }
        @keyframes steps { from { box-shadow: black 10px 0 } to { box-shadow: lime 10px 0 } }
        @keyframes size { from { width: 0; height: 10% } to { width: 200px; height: 30% } }
        @keyframes mixed { from { width: 50px } to { width: 50% } }
        #shadow { animation: shadow 2s linear -1s both }
        #steps { animation: steps 10s steps(2, start) both }
        #size { animation: size 2s linear -1s }
        #mixed { animation: mixed 2s linear -1s }",
    );
    let style = |id| by_id(&document, &styled, id);
    let shadows = &style("shadow").box_shadow;
    assert_eq!(shadows.len(), 2);
    let first: &BoxShadow = &shadows[0];
    assert_eq!((first.x, first.y, first.blur), (120.0, 10.0, 5.0));
    assert_eq!(first.color.map(bytes), Some([0, 128, 128, 255]));
    assert_eq!(
        shadows[1].color.map(bytes),
        Some([255, 0, 0, 128]),
        "from a transparent shadow"
    );
    // `steps(2, start)` has jumped by the first step at the start of the animation.
    assert_eq!(
        style("steps").box_shadow[0].color.map(bytes),
        Some([0, 128, 0, 255])
    );
    assert_eq!(style("size").width, Size::Length(LengthPercentage::Px(100.0)));
    assert!(
        matches!(style("size").height, Size::Length(LengthPercentage::Percent(fraction)) if (fraction - 0.2).abs() < 1e-6)
    );
    let Size::Length(mixed) = &style("mixed").width else {
        panic!("{:?}", style("mixed").width);
    };
    assert!((mixed.resolve(200.0) - 75.0).abs() < 0.01);
}

#[test]
fn puts_elements_on_layers() {
    use crate::values::{Layer, LayerShown};
    let (document, styled) = styles(
        "<p id=a></p><p id=b></p><p id=c></p><p id=d></p><div id=e class=on><p id=f></p></div>",
        "#a { -fagus-layer: 'Notes' } #b { -fagus-layer: \"Notes\" SCREEN } #c { -fagus-layer: 'Copy' print }
         #d { -fagus-layer: Notes } .on { -fagus-layer: 'A' }
         #f { -fagus-layer: none }",
    );
    let layer = |id: &str| by_id(&document, &styled, id).layer.as_deref().cloned();
    let notes = |shown| {
        Some(Layer {
            name: "Notes".into(),
            shown,
        })
    };
    assert_eq!(layer("a"), notes(LayerShown::Always));
    assert_eq!(layer("b"), notes(LayerShown::Screen));
    assert_eq!(
        layer("c"),
        Some(Layer {
            name: "Copy".into(),
            shown: LayerShown::Print,
        })
    );
    // A name is a string.
    assert_eq!(layer("d"), None);
    // Not inherited: what the element paints is on its layer, children too.
    assert_eq!(layer("f"), None);
    assert!(by_id(&document, &styled, "e").is_stacking_context());
}

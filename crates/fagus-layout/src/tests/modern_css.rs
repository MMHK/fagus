//! Layout of documents written with modern CSS as Chrome prints them: font-relative lengths,
//! aspect ratios and percentage heights in flex and grid containers, generated flex and grid
//! containers, list markers beside padded list items, grids that break across pages and the
//! text of glyph clusters. The numbers are Chrome's, measured with the same font.

use std::path::Path;

use fagus_css::{MediaEnvironment, Origin, RuleSet, Stylesheet};
use fagus_dom::Document;
use fagus_text::FontDb;

use super::{find, render_document};
use crate::{Fragment, FragmentKind, Images, Page, StyleFonts, layout};

/// Like `render_document`, with styles computed with the fonts' metrics for font-relative
/// lengths.
fn render_with_metrics(html: &str) -> (Document, Vec<Page>) {
    let mut fonts = FontDb::new();
    fonts
        .add_font_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fonts"))
        .expect("test fonts");
    let document = Document::parse_html(html);
    let ua = Stylesheet::parse(fagus_style::USER_AGENT_CSS, Origin::UserAgent, "ua.css");
    let css: String = document
        .descendants(fagus_dom::NodeId::DOCUMENT)
        .filter(|&node| document.element(node).is_some_and(|data| data.is("style")))
        .map(|node| document.text_content(node))
        .collect();
    let author = Stylesheet::parse(&css, Origin::Author, "style");
    let environment = MediaEnvironment::default();
    let rules = RuleSet::new(&[&ua, &author], &environment);
    let styled = fagus_style::compute_styles_with(
        &document,
        &rules,
        (environment.width, environment.height),
        &fagus_style::ContainerSizes::default(),
        &StyleFonts(&fonts),
    );
    let pages = layout(&document, &styled, &rules, &fonts, &Images::default());
    (document, pages)
}

/// The text runs below `fragment` with their positions on the page.
fn texts(fragment: &Fragment, origin: (f32, f32), out: &mut Vec<(String, f32, f32)>) {
    let x = origin.0 + fragment.rect.x.to_px();
    let y = origin.1 + fragment.rect.y.to_px();
    if let FragmentKind::Text(run) = &fragment.kind {
        out.push((run.text.clone(), x, y));
    }
    for child in &fragment.children {
        texts(child, (x, y), out);
    }
}

/// Where the run that starts with `text` is on `page`.
fn text_at(page: &Page, text: &str) -> (f32, f32) {
    let mut found = Vec::new();
    for fragment in &page.fragments {
        texts(fragment, (0.0, 0.0), &mut found);
    }
    found
        .into_iter()
        .find(|(run, ..)| run.trim_start().starts_with(text))
        .map_or_else(|| panic!("no run {text:?}"), |(_, x, y)| (x, y))
}

#[test]
fn sizes_font_relative_lengths_by_the_first_available_font() {
    let (document, pages) = render_with_metrics(
        "<style>body { font: 16px/20px Inter } div { height: 1px }
         .ch { width: 10ch } .ex { width: 10ex } .cap { width: 10cap } .ic { width: 10ic }
         .big { font-size: 20px } .big span { display: block; font-size: 2ch; width: 1em }</style>
         <div class=ch></div><div class=ex></div><div class=cap></div><div class=ic></div>
         <div class=big><span class=nested></span></div>",
    );
    let width = |class: &str| {
        find(&pages[0], &document, class)
            .expect("box")
            .2
            .rect
            .width
            .to_px()
    };
    // Chrome: the advance of Inter's "0", its x-height and its cap height; an em for the
    // ideograph Inter lacks.
    assert!((width("ch") - 100.94).abs() < 0.05, "{}", width("ch"));
    assert!((width("ex") - 87.34).abs() < 0.05, "{}", width("ex"));
    assert!((width("cap") - 116.41).abs() < 0.05, "{}", width("cap"));
    assert!((width("ic") - 160.0).abs() < 0.05, "{}", width("ic"));
    // `font-size` in `ch` refers to the parent's font.
    assert!((width("nested") - 25.23).abs() < 0.05, "{}", width("nested"));
}

#[test]
fn aligns_items_in_a_height_from_an_aspect_ratio() {
    let (document, pages) = render_document(
        "<style>body { margin: 0; font: 12px/1.2 Inter }
         .box { width: 100px; aspect-ratio: 1 }
         .column { display: flex; flex-direction: column; justify-content: space-between }
         .center { display: grid; place-items: center } .row { display: flex; align-items: center }</style>
         <div class='box column a'><span>A1</span><span class=last>A2</span></div>
         <div class='box center b'><span class=b1>B1</span></div>
         <div class='box row c'><span class=c1>C1</span></div>",
    );
    let top = |class: &str| find(&pages[0], &document, class).expect("box").1;
    let height = |class: &str| {
        find(&pages[0], &document, class)
            .expect("box")
            .2
            .rect
            .height
            .to_px()
    };
    // The squares are 100 pixels high; the last item ends at the bottom of its square, the others
    // are centered in theirs.
    assert_eq!((height("a"), height("b"), height("c")), (100.0, 100.0, 100.0));
    assert!((top("last") + height("last") - top("a") - 100.0).abs() < 0.02);
    for (square, item) in [("b", "b1"), ("c", "c1")] {
        let offset = top(item) - top(square);
        assert!(
            (offset - (100.0 - height(item)) / 2.0).abs() < 0.02,
            "{item}: {offset}"
        );
    }
}

#[test]
fn resolves_percentage_heights_of_grid_items_against_their_areas() {
    let (document, pages) = render_document(
        "<style>body { margin: 0 } .grid { display: grid; grid-template-columns: 40% 1fr; width: 400px }
         .pct { height: 100% } .half { height: 50% } .tall { height: 150px }</style>
         <div class=grid><div class=pct></div><div class=tall></div></div>
         <div class=grid><div class=half></div><div class=tall></div></div>",
    );
    let height = |class: &str| {
        find(&pages[0], &document, class)
            .expect("box")
            .2
            .rect
            .height
            .to_px()
    };
    assert_eq!(height("pct"), 150.0);
    assert_eq!(height("half"), 75.0);
}

#[test]
fn wraps_the_text_of_generated_flex_and_grid_containers_in_an_item() {
    let (_, pages) = render_document(
        "<style>body { margin: 0; font: 12px/1.2 Inter }
         .g::before { content: 'B'; display: grid; place-items: center; width: 50px; height: 30px }
         .f::before { content: 'D'; display: flex; justify-content: flex-end; width: 50px }</style>
         <div class=g></div><div class=f></div>",
    );
    let (left, top) = (
        pages[0].fragments[0].rect.x.to_px(),
        pages[0].fragments[0].rect.y.to_px(),
    );
    let (b, b_y) = text_at(&pages[0], "B");
    let (d, _) = text_at(&pages[0], "D");
    // Centered in the 50 by 30 pixels of the grid, as in Chrome (B is 8.2 pixels wide).
    assert!((b - left - 20.9).abs() < 0.3, "{b}");
    assert!(b_y - top > 5.0, "{b_y}");
    // At the end of the flex row.
    assert!(d - left > 40.0, "{d}");
}

#[test]
fn places_outside_markers_beside_the_border_box_of_the_list_item() {
    let (_, pages) = render_document(
        "<style>body { margin: 0; font: 12px/1.4 Inter } ul, ol { margin: 0; padding-left: 40px }
         .s { list-style: '\u{2212}  ' }</style>
         <ol><li>I plain</li><li style='padding-left: 20px'>J padded</li></ol>
         <ul class=s><li>E plain</li><li style='margin-left: -4px; padding-left: 4px'>G neg</li>
         <li style='border-left: 10px solid'>H border</li></ul>",
    );
    let marker = |text: &str| text_at(&pages[0], text).0;
    // Chrome: markers end where the list item's border box starts, whatever its padding and
    // border (their boxes, as wide as their text rounded up to layout units).
    assert!((marker("2.") - marker("1.")).abs() < 0.01);
    let end = marker("\u{2212}") + marker_width(&pages[0]);
    assert!(end <= marker("E plain") && marker("E plain") - end <= 1.0 / 64.0);
    let mut found = Vec::new();
    for fragment in &pages[0].fragments {
        texts(fragment, (0.0, 0.0), &mut found);
    }
    let minus: Vec<f32> = found
        .iter()
        .filter(|(run, ..)| run.starts_with('\u{2212}'))
        .map(|(_, x, _)| *x)
        .collect();
    assert_eq!(minus.len(), 3);
    assert!((minus[1] - (minus[0] - 4.0)).abs() < 0.01, "{minus:?}");
    assert!((minus[2] - minus[0]).abs() < 0.01, "{minus:?}");
}

/// The width of the first marker run starting with a minus sign on `page`.
fn marker_width(page: &Page) -> f32 {
    fn walk(fragment: &Fragment) -> Option<f32> {
        if let FragmentKind::Text(run) = &fragment.kind
            && run.marker
            && run.text.starts_with('\u{2212}')
        {
            return Some(fragment.rect.width.to_px());
        }
        fragment.children.iter().find_map(walk)
    }
    page.fragments.iter().find_map(walk).expect("marker")
}

#[test]
fn breaks_a_grid_before_its_first_row_inside_its_top_padding() {
    let (_, pages) = render_document(
        "<style>@page { size: 400px 300px; margin: 20px } body { margin: 0; font: 12px/20px Inter }
         .spacer { height: 250px } .box { border-top: 1px dashed; padding-top: 16px }
         .grid { display: grid } .flex { display: flex } .next { break-before: page }</style>
         <div class=spacer></div><div class='box grid'><div>B1</div><div>B2</div></div>
         <div class='spacer next'></div><div class='box flex'><div>C1</div></div>
         <div class='spacer next' style='height: 236px'></div><div class='box grid'><div>E1</div></div>",
    );
    assert_eq!(pages.len(), 6);
    // Chrome: the border and 9 pixels of the padding fill the page, the other 7 go on before
    // the first row.
    // Runs start 2 pixels below the top of their lines.
    assert!((text_at(&pages[1], "B1").1 - 29.0).abs() < 0.01);
    // A flex container moves to the next page as a whole.
    assert!((text_at(&pages[3], "C1").1 - 39.0).abs() < 0.01);
    // Where the padding fits, only the row moves.
    assert!((text_at(&pages[5], "E1").1 - 22.0).abs() < 0.01);
}

#[test]
fn moves_a_container_whose_bottom_padding_does_not_fit_to_the_next_page() {
    let (_, pages) = render_document(
        "<style>@page { size: 400px 300px; margin: 20px } body { margin: 0; font: 10pt/1.5 Inter }
         .spacer { height: 200px } .row { padding: 5px 0; border-bottom: 1px solid }</style>
         <div class=spacer></div>
         <div><div class=row style='display: grid'>C1</div><div class=row style='display: grid'>C2</div></div>",
    );
    // C2's line fits on the first page, its bottom padding and border do not: Chrome moves it.
    assert_eq!(pages.len(), 2);
    // Its line starts below its top padding, as C1's line starts 225 pixels down the first page.
    let offset = text_at(&pages[0], "C1").1 - 225.0;
    assert!((text_at(&pages[1], "C2").1 - offset - 25.0).abs() < 0.01);
}

/// The first text run below `fragment`.
fn first_run(fragment: &Fragment) -> Option<&crate::TextRun> {
    match &fragment.kind {
        FragmentKind::Text(run) => Some(run),
        _ => fragment.children.iter().find_map(first_run),
    }
}

#[test]
fn gives_all_glyphs_of_a_cluster_its_whole_text() {
    let (_, pages) = render_document("<style>body { font: 16px Inter }</style><p>q\u{301}x</p>");
    let run = pages[0].fragments.iter().find_map(first_run).expect("run");
    let clusters: Vec<(u32, u32)> = run.glyphs.iter().map(|glyph| glyph.cluster).collect();
    // The letter and its combining accent are one cluster of three bytes: the PDF maps their
    // glyphs to it together, so that text extraction finds it without gaps.
    assert_eq!(clusters, [(0, 3), (0, 3), (3, 4)]);
}

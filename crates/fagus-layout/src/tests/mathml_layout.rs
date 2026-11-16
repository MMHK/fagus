//! `MathML` formulas laid out as Chrome lays them out, in STIX Two Math (`tests/webfonts`): the
//! numbers are those of Blink's algorithms with the font's `MATH` table.

use fagus_dom::Document;
use fagus_text::FontDb;

use super::{render_document_in, test_fonts};
use crate::fragment::{Fragment, FragmentKind, Page};
use crate::image::Images;

/// The test fonts with STIX Two Math.
fn fonts() -> FontDb {
    let mut fonts = test_fonts();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/webfonts/STIXTwoMath-Regular.woff2");
    let data = fagus_text::unpack(std::fs::read(path).expect("font")).expect("unpacked");
    fonts.add_font_data(data).expect("STIX Two Math");
    fonts
}

fn render(body: &str) -> (Document, Vec<Page>) {
    render_document_in(
        &format!("<!DOCTYPE html><style>body {{ margin: 0; font: 16px/20px Inter }}</style>{body}"),
        &fonts(),
        |_| Images::default(),
    )
}

/// A run of text on a page: its origin (left end of its baseline) in CSS px, its size and text.
#[derive(Debug)]
struct Run {
    x: f32,
    baseline: f32,
    size: f32,
    text: String,
    glyph: u32,
}

/// The text runs and the boxes filled with a color (rules) of a page, in CSS px.
fn contents(page: &Page) -> (Vec<Run>, Vec<[f32; 4]>) {
    fn walk(fragment: &Fragment, origin: (f32, f32), runs: &mut Vec<Run>, rules: &mut Vec<[f32; 4]>) {
        let x = origin.0 + fragment.rect.x.to_px();
        let y = origin.1 + fragment.rect.y.to_px();
        match &fragment.kind {
            FragmentKind::Text(run) => runs.push(Run {
                x,
                baseline: y + run.baseline.to_px(),
                size: run.size,
                text: run.text.clone(),
                glyph: run.glyphs.first().map_or(0, |glyph| glyph.id),
            }),
            FragmentKind::Box {
                style, node: None, ..
            } if style.background_color.a > 0.0 => {
                rules.push([x, y, fragment.rect.width.to_px(), fragment.rect.height.to_px()]);
            }
            _ => {}
        }
        for child in &fragment.children {
            walk(child, (x, y), runs, rules);
        }
    }
    let (mut runs, mut rules) = (Vec::new(), Vec::new());
    for fragment in &page.fragments {
        walk(fragment, (0.0, 0.0), &mut runs, &mut rules);
    }
    (runs, rules)
}

fn run<'r>(runs: &'r [Run], text: &str) -> &'r Run {
    runs.iter()
        .find(|run| run.text == text)
        .unwrap_or_else(|| panic!("{text} in {runs:?}"))
}

#[test]
fn lays_out_fractions_around_the_math_axis() {
    let (_, pages) = render("<p>A <math><mfrac><mn>1</mn><mn>2</mn></mfrac></math></p>");
    let (runs, rules) = contents(&pages[0]);
    let (a, one, two) = (run(&runs, "A "), run(&runs, "1"), run(&runs, "2"));
    // The numerator and denominator move by STIX Two Math's minimum shifts (585 units, cut to
    // layout units) from the baseline of the text, in scripts' smaller size.
    assert!((a.baseline - one.baseline - 9.359_375).abs() < 0.02, "{runs:?}");
    assert!((two.baseline - a.baseline - 9.359_375).abs() < 0.02, "{runs:?}");
    assert!(one.size < 16.0 && (one.size - two.size).abs() < 0.01);
    // The bar is as thick as the font says (68 units) and lies on the math axis (258 units).
    assert_eq!(rules.len(), 1, "{rules:?}");
    let [_, top, _, height] = rules[0];
    assert!((height - 1.078).abs() < 0.02, "{height}");
    assert!((a.baseline - top - 4.125).abs() < 0.02, "{top}");
}

#[test]
fn centers_block_formulas_and_maps_letters_to_italics() {
    let (_, pages) = render("<div style='width: 400px'>A<math display=block><mi>x</mi></math></div>");
    let (runs, _) = contents(&pages[0]);
    let (a, x) = (run(&runs, "A"), run(&runs, "𝑥"));
    // 𝑥 is 559 units wide at 16px, centered in 400px.
    assert!((x.x - a.x - (400.0 - 8.953_125) / 2.0).abs() < 0.02, "{runs:?}");
}

#[test]
fn shrinks_scripts_and_spaces_operators() {
    let (_, pages) = render("<p><math><msup><mi>x</mi><mn>2</mn></msup><mo>+</mo><mn>1</mn></math></p>");
    let (runs, _) = contents(&pages[0]);
    let (x, two, plus) = (run(&runs, "𝑥"), run(&runs, "2"), run(&runs, "+"));
    // The script is a level deeper: 0.71 of the size without a `MATH` table in the cascade.
    assert!((two.size - 16.0 * 0.71).abs() < 0.05, "{}", two.size);
    assert!(two.baseline < x.baseline - 5.0);
    // `+` has medium spaces (4/18 em) on either side.
    assert!(plus.x > two.x, "{runs:?}");
    let one = run(&runs, "1");
    let plus_width = 720.0 * 16.0 / 1000.0;
    assert!((one.x - plus.x - plus_width - 3.546_875).abs() < 0.05, "{runs:?}");
}

#[test]
fn stretches_fences_to_their_content() {
    let (_, pages) = render(
        "<p><math><mo>(</mo><mi>x</mi><mo>)</mo></math> \
         <math><mo>(</mo><mfrac><mfrac><mn>1</mn><mn>2</mn></mfrac><mn>3</mn></mfrac><mo>)</mo></math></p>",
    );
    let (runs, _) = contents(&pages[0]);
    let parentheses: Vec<&Run> = runs.iter().filter(|run| run.text == "(").collect();
    assert_eq!(parentheses.len(), 2, "{runs:?}");
    // Around a letter the parenthesis keeps its glyph; around a tall fraction it takes a larger
    // variant.
    assert_ne!(parentheses[0].glyph, parentheses[1].glyph);
}

#[test]
fn shows_only_the_first_child_of_semantics() {
    let (_, pages) = render(
        "<p><math><semantics><mi>y</mi><annotation encoding='application/x-tex'>y</annotation>\
         </semantics></math></p>",
    );
    let (runs, _) = contents(&pages[0]);
    assert!(runs.iter().any(|run| run.text == "𝑦"), "{runs:?}");
    assert!(!runs.iter().any(|run| run.text == "y"), "{runs:?}");
}

#[test]
fn lays_out_tables_centered_on_the_axis() {
    let (_, pages) = render(
        "<p>A <math><mtable><mtr><mtd><mn>1</mn></mtd><mtd><mn>0</mn></mtd></mtr>\
         <mtr><mtd><mn>0</mn></mtd><mtd><mn>1</mn></mtd></mtr></mtable></math></p>",
    );
    let (runs, _) = contents(&pages[0]);
    let numbers: Vec<&Run> = runs
        .iter()
        .filter(|run| run.text == "1" || run.text == "0")
        .collect();
    assert_eq!(numbers.len(), 4);
    // Two rows of two cells, the columns as wide as a digit and 0.4em of padding on each side.
    assert!(
        (numbers[1].x - numbers[0].x - (7.921_875 + 2.0 * 6.390_625)).abs() < 0.05,
        "{runs:?}"
    );
    assert!((numbers[2].baseline - numbers[0].baseline).abs() > 10.0);
    // The middle of the table is on the math axis, above the baseline of the text: a row on
    // either side.
    let axis = run(&runs, "A ").baseline - 4.125;
    assert!(
        numbers[0].baseline < axis && numbers[2].baseline > axis,
        "{runs:?}"
    );
    let middle = f32::midpoint(numbers[0].baseline, numbers[2].baseline);
    assert!((middle - axis).abs() < 6.0, "{runs:?}");
}

#[test]
fn reads_alternative_texts_of_formulas() {
    let document = Document::parse_html(
        "<math alttext='a squared'><mi>a</mi></math>\
         <math><semantics><mi>b</mi><annotation encoding='application/x-tex'> b^2 </annotation></semantics></math>\
         <math><mi>c</mi><mo>+</mo><mn>1</mn></math>",
    );
    let maths: Vec<_> = document
        .descendants(fagus_dom::NodeId::DOCUMENT)
        .filter(|&node| document.element(node).is_some_and(crate::math::is_math))
        .collect();
    let alt = |index: usize| crate::math::alternative_text(&document, maths[index]);
    assert_eq!(alt(0).as_deref(), Some("a squared"));
    assert_eq!(alt(1).as_deref(), Some("b^2"));
    assert_eq!(alt(2).as_deref(), Some("c + 1"));
}

#[test]
fn lays_out_deeply_nested_formulas() {
    // Rows of nothing but a stretchy operator lay out their content twice: without keeping
    // layouts this would take 2^60 steps. Nesting beyond what is laid out is left out.
    let nested = |depth: usize| {
        format!(
            "<p><math>{}<mo>(</mo>{}</math></p>",
            "<mrow>".repeat(depth),
            "</mrow>".repeat(depth)
        )
    };
    let (_, pages) = render(&nested(60));
    assert!(contents(&pages[0]).0.iter().any(|run| run.text == "("));
    let (_, pages) = render(&nested(400));
    assert_eq!(pages.len(), 1);
}

#[test]
fn measures_the_ink_of_tokens_by_their_own_characters() {
    // Chrome measures `<mi>x</mi>` by the ink of "x", not of the italic it draws: both boxes
    // are as high, and the sum of 11pt is 733/64 px high above its baseline, not 734/64 (its
    // outline scaled from font units, as Core Text scales it).
    let (_, pages) = render(
        "<p><math><mi mathbackground='red'>x</mi><mi mathvariant='normal' mathbackground='blue'>x</mi>\
         <mo mathbackground='green' style='font-size: 14.6667px'>∑</mo></math></p>",
    );
    let (runs, rules) = contents(&pages[0]);
    assert_eq!(rules.len(), 3, "{rules:?}");
    assert_eq!(
        (rules[0][1], rules[0][3]),
        (rules[1][1], rules[1][3]),
        "{rules:?}"
    );
    let sum = run(&runs, "∑");
    assert!(
        (sum.baseline - rules[2][1] - 733.0 / 64.0).abs() < 0.001,
        "{rules:?} {runs:?}"
    );
}

#[test]
fn mirrors_right_to_left_formulas() {
    let (_, pages) = render(
        "<p><math><mi>a</mi><mo>+</mo><mo stretchy='false'>(</mo></math> \
         <math dir='rtl'><mi>b</mi><mo>+</mo><mo stretchy='false'>(</mo><msup><mi>c</mi><mn>2</mn></msup></math></p>",
    );
    let (runs, _) = contents(&pages[0]);
    let (b, c, two) = (run(&runs, "𝑏"), run(&runs, "𝑐"), run(&runs, "2"));
    // Right to left: the first child rightmost, the superscript left of its base.
    assert!(b.x > c.x && two.x < c.x, "{runs:?}");
    // The parenthesis is mirrored.
    let parens: Vec<&Run> = runs.iter().filter(|run| run.text == "(").collect();
    assert_eq!(parens.len(), 2);
    assert_ne!(parens[0].glyph, parens[1].glyph);
}

#[test]
fn lays_out_cells_that_span_rows_and_columns() {
    let (_, pages) = render(
        "<p><math><mtable>\
         <mtr><mtd rowspan='2' mathbackground='red'><mi>A</mi></mtd><mtd><mi>b</mi></mtd><mtd><mi>c</mi></mtd></mtr>\
         <mtr><mtd><mi>d</mi></mtd><mtd><mi>e</mi></mtd></mtr>\
         <mtr><mtd columnspan='3' mathbackground='blue'><mi>f</mi></mtd></mtr></mtable></math></p>",
    );
    let (runs, rules) = contents(&pages[0]);
    let (b, d, e) = (run(&runs, "𝑏"), run(&runs, "𝑑"), run(&runs, "𝑒"));
    // The second row starts past the cell that spans down into it.
    assert!(
        (d.x - b.x).abs() < 0.5 && e.x > d.x && d.baseline > b.baseline,
        "{runs:?}"
    );
    // The spanning cells fill their slots: two rows high, three columns wide.
    let [a_cell, f_cell] = [rules[0], rules[1]];
    assert!((f_cell[1] - (a_cell[1] + a_cell[3])).abs() < 0.01, "{rules:?}");
    assert!(f_cell[2] > a_cell[2] * 2.0, "{rules:?}");
}

#[test]
fn sizes_math_boxes_with_css() {
    let (_, pages) = render(
        "<p><math><mi style='width: 40px; margin-left: 10px; background: red'>x</mi><mo>+</mo>\
         <mn style='height: 30px; background: blue'>2</mn></math></p>",
    );
    let (runs, rules) = contents(&pages[0]);
    let (x, plus) = (run(&runs, "𝑥"), run(&runs, "+"));
    assert!((rules[0][2] - 40.0).abs() < 0.01, "{rules:?}");
    assert!((x.x - rules[0][0]).abs() < 0.01, "{rules:?} {runs:?}");
    // The `+` follows the 40 px box and its space (4/18 em).
    assert!(
        (plus.x - (rules[0][0] + 40.0 + 16.0 * 4.0 / 18.0)).abs() < 0.1,
        "{rules:?} {runs:?}"
    );
    // The number's box is 30 px high from the top of its ink.
    assert!((rules[1][3] - 30.0).abs() < 0.01, "{rules:?}");
}

#[test]
fn keeps_decorations_of_the_text_around_out_of_formulas() {
    fn decorations(fragment: &Fragment, text: &str) -> Option<usize> {
        if let FragmentKind::Text(run) = &fragment.kind
            && run.text == text
        {
            return Some(run.style.decorations.len());
        }
        fragment
            .children
            .iter()
            .find_map(|child| decorations(child, text))
    }
    let (_, pages) = render(
        "<p style='text-decoration: underline'>a <math><mi>z</mi><mi style='text-decoration: overline'>w</mi></math></p>",
    );
    let page = &pages[0].fragments[0];
    assert_eq!(decorations(page, "a "), Some(1));
    assert_eq!(decorations(page, "𝑧"), Some(0));
    assert_eq!(decorations(page, "𝑤"), Some(1));
}

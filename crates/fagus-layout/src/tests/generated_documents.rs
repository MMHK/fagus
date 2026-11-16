//! What documents that office programs, document generators and CSS frameworks write need
//! (`LibreOffice` lists of paragraphs, pandoc footnotes after the last word, justified text with
//! no-break spaces, daisyUI's stats in a grid that flows by columns), with Chrome's numbers.

use super::{line_texts, render};
use crate::fragment::{Fragment, FragmentKind};

/// The runs of text below `fragment`: their left edge and baseline on the page, width and text.
fn runs(fragment: &Fragment) -> Vec<(f32, f32, f32, String)> {
    fn walk(fragment: &Fragment, origin: (f32, f32), out: &mut Vec<(f32, f32, f32, String)>) {
        let (x, y) = (
            origin.0 + fragment.rect.x.to_px(),
            origin.1 + fragment.rect.y.to_px(),
        );
        if let FragmentKind::Text(run) = &fragment.kind {
            let width = run.glyphs.iter().map(|glyph| glyph.advance).sum();
            out.push((x, y + run.baseline.to_px(), width, run.text.clone()));
        }
        for child in &fragment.children {
            walk(child, (x, y), out);
        }
    }
    let mut out = Vec::new();
    walk(fragment, (0.0, 0.0), &mut out);
    out
}

/// The baseline of the first run that starts with `text`.
fn baseline(runs: &[(f32, f32, f32, String)], text: &str) -> f32 {
    runs.iter()
        .find(|run| run.3.starts_with(text))
        .map_or_else(|| panic!("no run {text:?} in {runs:?}"), |run| run.1)
}

#[test]
fn content_moves_below_a_marker_that_reaches_higher_than_its_first_line() {
    // LibreOffice puts a paragraph of a smaller font into each list item: the marker in the
    // item's font of 24px reaches 23px above its baseline, the paragraph's first line only 11px,
    // so Chrome moves the paragraph 12px down and the items are 12px + 13.8px apart (Blink's
    // `UnpositionedListMarker::AddToBox`; printed on whole pixels, 26px). Items whose line height
    // lowers the marker's ascent move less, and a paragraph in the item's own font does not move.
    let pages = render(
        "<style>body { font: 24px Inter; margin: 0 } ol { margin: 0 } p { font: 12px/1.15 Inter; margin: 0 }
           .tight { line-height: 1 } .same p { font: 24px Inter }</style>
         <ol><li><p>Eins</p><li><p>Zwei</p></ol>
         <ol class=tight><li><p>Drei</p><li><p>Vier</p></ol>
         <ol class=same><li><p>Fünf</p><li><p>Sechs</p></ol>",
    );
    let runs = runs(&pages[0].fragments[0]);
    let step = |a: &str, b: &str| baseline(&runs, b) - baseline(&runs, a);
    assert!((step("Eins", "Zwei") - 25.8).abs() < 0.05, "{runs:?}");
    assert!((step("Drei", "Vier") - 22.8).abs() < 0.05, "{runs:?}");
    assert!((step("Fünf", "Sechs") - 29.0).abs() < 0.05, "{runs:?}");
    // The marker stays on the baseline of the paragraph's first line.
    assert!(
        (baseline(&runs, "1.") - baseline(&runs, "Eins")).abs() < 0.05,
        "{runs:?}"
    );
}

#[test]
fn hyphenates_a_last_word_that_goes_on_in_the_next_element_only_where_it_starts_a_line() {
    // Blink finds the end of the last word in the whole paragraph: "Juliwerts" followed by a
    // footnote reference, a link or a period in an element is the last word and stays whole,
    // unless the element starts with the word, where Blink looks for its start no further back.
    let lines = |content: &str| {
        let pages = render(&format!(
            "<html lang=de><style>body {{ font: 16px Inter; margin: 0 }}
               p {{ width: 168px; hyphens: auto; margin: 0 }}</style><p>{content}</p></html>"
        ));
        let mut lines = Vec::new();
        line_texts(&pages[0].fragments[0], &mut lines);
        lines
    };
    for content in [
        "Vierfachen des Juliwerts.",
        "Vierfachen des Juliwerts.<a href='#fn'>x</a>",
        "Vierfachen des Juliwerts<sup>2</sup>.",
        "Vierfachen des Juliwerts<span>.</span>",
    ] {
        assert_eq!(lines(content).len(), 2, "{content}");
        assert!(lines(content)[1].starts_with("Juliwerts"), "{content}");
    }
    for content in [
        "Vierfachen des <span>Juliwerts</span>.",
        "Vierfachen des Juliwerts, und",
    ] {
        assert_eq!(lines(content)[0], "Vierfachen des Juli\u{AD}", "{content}");
    }
}

#[test]
fn justification_widens_no_break_spaces_too() {
    // "Weg&nbsp;C": Chrome widens the no-break space like the other spaces of the line.
    let pages = render(
        "<style>body { font: 16px Inter; margin: 0 } p { width: 300px; text-align: justify; margin: 0 }</style>
         <p>Die Leitung im Weg\u{a0}C wurde erneuert, die Kosten blieben unter dem Ansatz.</p>",
    );
    let runs = runs(&pages[0].fragments[0]);
    let line: Vec<_> = runs
        .iter()
        .filter(|run| (run.1 - runs[0].1).abs() < 0.01)
        .collect();
    let text: String = line.iter().map(|run| run.3.as_str()).collect();
    assert!(text.contains("Weg\u{a0}C"), "{text:?}");
    // The line is justified to its end.
    let end = line.iter().map(|run| run.0 + run.2).fold(0.0, f32::max);
    assert!((end - 300.0).abs() < 0.05 || text.ends_with(' '), "{runs:?}");
    // Each space and the no-break space are as wide.
    let pages_plain = render(
        "<style>body { font: 16px Inter; margin: 0 } p { width: 300px; text-align: justify; margin: 0 }</style>
         <p>Die Leitung im Weg C wurde erneuert, die Kosten blieben unter dem Ansatz.</p>",
    );
    let plain = runs_of_first_line(&pages_plain[0].fragments[0]);
    let nbsp = runs_of_first_line(&pages[0].fragments[0]);
    assert_eq!(plain.len(), nbsp.len());
    for (a, b) in plain.iter().zip(&nbsp) {
        assert!((a - b).abs() < 0.05, "{plain:?} {nbsp:?}");
    }
}

/// The left edges of the glyphs of the first line below `fragment`.
fn runs_of_first_line(fragment: &Fragment) -> Vec<f32> {
    fn walk(fragment: &Fragment, x: f32, out: &mut Vec<f32>) -> bool {
        let x = x + fragment.rect.x.to_px();
        if matches!(fragment.kind, FragmentKind::Line { .. }) {
            glyphs(fragment, x - fragment.rect.x.to_px(), out);
            return true;
        }
        fragment.children.iter().any(|child| walk(child, x, out))
    }
    fn glyphs(fragment: &Fragment, x: f32, out: &mut Vec<f32>) {
        let x = x + fragment.rect.x.to_px();
        if let FragmentKind::Text(run) = &fragment.kind {
            let mut pen = x;
            for glyph in &run.glyphs {
                out.push(pen);
                pen += glyph.advance;
            }
        }
        for child in &fragment.children {
            glyphs(child, x, out);
        }
    }
    let mut out = Vec::new();
    walk(fragment, 0.0, &mut out);
    out
}

#[test]
fn grids_flow_by_columns() {
    // daisyUI's `.stats`: `grid-auto-flow: column` without a template puts each item into a new
    // column of one row; with explicit rows, the cursor goes down the column first.
    let pages = render(
        "<style>body { font: 16px/20px Inter; margin: 0 } .g { display: grid; grid-auto-flow: column; width: 600px }
           .r { grid-template-rows: 20px 20px }</style>
         <div class=g><div>A1</div><div>A2</div><div>A3</div></div>
         <div class='g r'><div>B1</div><div>B2</div><div>B3</div><div style='grid-row: 1'>B4</div></div>",
    );
    let runs = runs(&pages[0].fragments[0]);
    let at = |text: &str| {
        runs.iter()
            .find(|run| run.3 == text)
            .map_or_else(|| panic!("no {text} in {runs:?}"), |run| (run.0, run.1))
    };
    // Three columns in one row, sharing the free space (Chrome: A2 at 197.44px, A3 at 398.66px).
    assert_eq!(at("A2").1, at("A1").1);
    assert_eq!(at("A3").1, at("A1").1);
    assert!((at("A2").0 - at("A1").0 - 197.44).abs() < 0.05, "{runs:?}");
    assert!((at("A3").0 - at("A1").0 - 398.66).abs() < 0.05, "{runs:?}");
    // B1 and B2 fill the first column, B3 starts the second, and B4, locked to the first row,
    // goes to the first free column of that row.
    assert_eq!(at("B1").0, at("B2").0);
    assert!((at("B2").1 - at("B1").1 - 20.0).abs() < 0.05, "{runs:?}");
    assert_eq!(at("B3").1, at("B1").1);
    assert!(at("B3").0 > at("B1").0);
    assert_eq!(at("B4").1, at("B1").1);
    assert!(at("B4").0 > at("B3").0, "{runs:?}");
}

#[test]
fn spaces_after_a_box_that_overflows_its_line_stay_on_that_line() {
    // Bootstrap's and daisyUI's `w-full` inline grids and blocks with a border are wider than
    // the line: the space after each hangs at the end of its line, and the next box starts the
    // next line, without an empty line between them.
    let pages = render(
        "<style>body { font: 14px/20px Inter; margin: 0 } div { width: 300px }
           span { display: inline-block; width: 100%; border: 1px solid }</style>
         <div><span>A1</span> <span>A2</span> <span>A3</span></div>",
    );
    let runs = runs(&pages[0].fragments[0]);
    let baselines: Vec<f32> = ["A1", "A2", "A3"]
        .iter()
        .map(|text| baseline(&runs, text))
        .collect();
    assert!((baselines[1] - baselines[0] - 22.0).abs() < 0.05, "{runs:?}");
    assert!((baselines[2] - baselines[1] - 22.0).abs() < 0.05, "{runs:?}");
}

#[test]
fn scrolling_flex_and_grid_containers_keep_the_baseline_of_their_items() {
    // Unlike an inline block, whose baseline is its bottom margin edge when it scrolls, an
    // inline flex or grid container with `overflow` sits on the baseline of its items in Chrome.
    let pages = render(
        "<style>body { font: 14px Inter; margin: 0 } span.x { border: 1px solid; padding: 5px; overflow: hidden }</style>
         <div>a <span class=x style='display: inline-block'>B1</span> b</div>
         <div>c <span class=x style='display: inline-grid'>G1</span> d</div>
         <div>e <span class=x style='display: inline-flex; overflow: auto'>F1</span> f</div>",
    );
    let runs = runs(&pages[0].fragments[0]);
    assert!(baseline(&runs, "B1") < baseline(&runs, "a") - 1.0, "{runs:?}");
    assert!(
        (baseline(&runs, "G1") - baseline(&runs, "c")).abs() < 0.05,
        "{runs:?}"
    );
    assert!(
        (baseline(&runs, "F1") - baseline(&runs, "e")).abs() < 0.05,
        "{runs:?}"
    );
}

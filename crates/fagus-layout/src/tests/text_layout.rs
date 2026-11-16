//! Text and inline layout as Chrome does it: ruby annotations that move lines and initial
//! letters, `text-fit`, words hyphenated again where a line starts in them, tabs that hang or
//! move to the next line, the box of `::first-line` and markers of trimmed lines.

use super::{line_texts, render};
use crate::fragment::{Fragment, FragmentKind};

/// The text runs under `fragment`: their left edge, baseline, width, font size and text.
fn placed_runs(fragment: &Fragment) -> Vec<(f32, f32, f32, f32, String)> {
    fn walk(fragment: &Fragment, origin: (f32, f32), out: &mut Vec<(f32, f32, f32, f32, String)>) {
        let (x, y) = (
            origin.0 + fragment.rect.x.to_px(),
            origin.1 + fragment.rect.y.to_px(),
        );
        if let FragmentKind::Text(run) = &fragment.kind {
            let width = run.glyphs.iter().map(|glyph| glyph.advance).sum();
            out.push((x, y + run.baseline.to_px(), width, run.size, run.text.clone()));
        }
        for child in &fragment.children {
            walk(child, (x, y), out);
        }
    }
    let mut out = Vec::new();
    walk(fragment, (0.0, 0.0), &mut out);
    out
}

/// The baseline of the first run with `text` in `runs`.
fn baseline(runs: &[(f32, f32, f32, f32, String)], text: &str) -> f32 {
    runs.iter()
        .find(|run| run.4.contains(text))
        .map_or(f32::NAN, |run| run.1)
}

#[test]
fn lets_a_raised_initial_letter_cover_ruby_annotations() {
    // The annotation of the first text line fits in the lines the letter raises the text by, so
    // the text stays where it is without ruby (Blink's `ComputeAnnotationBlockOffsetAdjustment`).
    let baselines = |content: &str| {
        let pages = render(&format!(
            "<style>@page {{ margin: 0 }} body {{ font: 20px/24px Inter; margin: 0 }}
               p {{ width: 230px; border: 1px solid }} p::first-letter {{ initial-letter: 3 raise }}</style>
             <p>{content}<br>def<br>ghi</p>"
        ));
        let runs = placed_runs(&pages[0].fragments[0]);
        (baseline(&runs, "xyz"), baseline(&runs, "def"))
    };
    let plain = baselines("Abc xyz");
    let ruby = baselines("Abc <ruby>xyz<rt>XYZ</rt></ruby>");
    assert_eq!(plain, ruby);
}

#[test]
fn moves_a_dropped_initial_letter_with_its_line() {
    let place = |content: &str| {
        let pages = render(&format!(
            "<style>@page {{ margin: 0 }} body {{ font: 20px/24px Inter; margin: 0 }}
               p {{ width: 230px; border: 1px solid }} p::first-letter {{ initial-letter: 3 }}</style>
             <p>{content}<br>def<br>ghi<br>jkl<br>mno</p>"
        ));
        let runs = placed_runs(&pages[0].fragments[0]);
        (baseline(&runs, "A"), baseline(&runs, "bc"))
    };
    let (plain_letter, plain_text) = place("Abc xyz");
    let (letter, text) = place("Abc <ruby>xyz<rt>XYZ</rt></ruby>");
    // The annotation moves the first line down, and the letter, which stands in it, with it.
    assert!(text > plain_text);
    assert!((letter - plain_letter - (text - plain_text)).abs() < 0.01);
}

#[test]
fn puts_lines_below_the_initial_letter_that_annotations_under_a_line_move_past() {
    let pages = render(
        "<style>@page { margin: 0 } body { font: 20px/24px Inter; margin: 0 }
           p { width: 230px } p::first-letter { initial-letter: 3 } rt { font-size: 80px }</style>
         <p>Abc <ruby style='ruby-position: under'>xyz<rt>X</rt></ruby><br>def<br>ghi<br>jkl</p>",
    );
    let runs = placed_runs(&pages[0].fragments[0]);
    let x = |text: &str| runs.iter().find(|run| run.4 == text).map(|run| run.0);
    // The lines after the tall annotation lie below the letter, at the start of the line.
    assert_eq!(x("def"), Some(0.0), "{runs:?}");
}

#[test]
fn grows_and_shrinks_text_to_fill_its_lines() {
    let pages = render(
        "<style>@page { margin: 0 } body { font: 16px Inter; margin: 0 } p { margin: 0 0 10px }
           .grow { width: 300px; text-fit: grow per-line-all }
           .shrink { width: 100px; white-space: nowrap; text-fit: shrink per-line-all }
           .consistent { width: 300px; white-space: pre; text-fit: grow }
           .float { width: 300px; text-fit: grow per-line-all }
           .float i { float: left }</style>
         <p class=grow>Quarterly report</p><p class=shrink>A rather long headline</p>
         <p class=consistent>Total due\nEUR 1,234.56 in all</p>
         <p class=float><i>x</i>Not fitted</p>",
    );
    let runs = placed_runs(&pages[0].fragments[0]);
    let run = |text: &str| {
        runs.iter()
            .find(|run| run.4.starts_with(text))
            .expect(text)
            .clone()
    };
    // Each line of text grows or shrinks to the width of the line, and its font with it.
    let grown = run("Quarterly");
    assert!((grown.2 - 300.0).abs() < 0.5, "{grown:?}");
    assert!(grown.3 > 30.0);
    let shrunk = run("A rather");
    assert!((shrunk.2 - 100.0).abs() < 0.5, "{shrunk:?}");
    assert!(shrunk.3 < 16.0);
    // By the factor of the line that needs the least, all lines alike.
    let (first, second) = (run("Total"), run("EUR"));
    assert!((first.3 - second.3).abs() < 1e-3);
    assert!((second.2 - 300.0).abs() < 0.5, "{second:?}");
    // The line grows with the text.
    assert!(second.1 - first.1 > 1.2 * first.3);
    // Not where the paragraph has floats.
    assert!((run("Not").3 - 16.0).abs() < 1e-3);
}

#[test]
fn hyphenates_the_rest_of_a_word_again_where_a_line_starts_in_it() {
    let pages = render(
        "<html lang=en><style>body { font: 16px Inter; margin: 0 }
           p { width: 1px; hyphens: auto; hyphenate-character: '' }</style>
         <p>implementation</p>",
    );
    let mut lines = Vec::new();
    line_texts(&pages[0].fragments[0], &mut lines);
    // "tation" alone has no hyphenation point, though "implementation" has one in it.
    assert_eq!(lines, ["im", "ple", "men", "tation"]);
}

#[test]
fn breaks_the_rest_of_a_word_where_it_alone_has_a_hyphenation_point() {
    let pages = render(
        "<html lang=de><style>body { font: 16px Inter; margin: 0 }
           p { width: 1px; hyphens: auto; hyphenate-character: '' }</style>
         <p>vollendet</p>",
    );
    let mut lines = Vec::new();
    line_texts(&pages[0].fragments[0], &mut lines);
    // "vollendet" breaks after "voll" alone, but "endet" that starts a line breaks after "en",
    // as Chrome hyphenates it again there.
    assert_eq!(lines, ["voll", "en", "det"]);
}

#[test]
fn leaves_tabs_that_hang_out_of_the_min_content_width() {
    let pages = render(
        "<style>body { font: 16px Inter; margin: 0 }
           div { width: min-content; white-space: pre-wrap; margin-bottom: 10px }
           .anywhere { overflow-wrap: anywhere }</style>
         <div>&#9;&#9;<span>XX</span></div><div>XX</div>
         <div class=anywhere>&#9;&#9;XX</div><div>X</div>",
    );
    let widths: Vec<f32> = pages[0].fragments[0].children[0]
        .children
        .iter()
        .map(|div| div.rect.width.to_px())
        .collect();
    // As wide as the text: the tabs hang at the end of their line.
    assert_eq!(widths[0], widths[1], "{widths:?}");
    assert_eq!(widths[2], widths[3], "{widths:?}");
}

#[test]
fn breaks_before_a_tab_that_starts_preformatted_text() {
    let lines = |content: &str| {
        let pages = render(&format!(
            "<style>body {{ font: 16px Inter; margin: 0 }} div {{ width: 30px }}</style><div>{content}</div>"
        ));
        let mut lines = Vec::new();
        line_texts(&pages[0].fragments[0], &mut lines);
        lines
    };
    // The tab cannot hang where lines do not wrap: it goes to the next line.
    assert_eq!(
        lines("X <span style='white-space: pre'>&#9;Y</span> X"),
        ["X", "Y", "X"]
    );
    // Where they wrap, it hangs on its line.
    assert_eq!(
        lines("X <span style='white-space: pre-wrap'>&#9;</span> X"),
        ["X", "X"]
    );
}

/// The width and height of the boxes in the lines under `fragment`, and whether they are blue.
fn line_boxes(fragment: &Fragment, out: &mut Vec<(f32, f32, bool)>) {
    for child in &fragment.children {
        if matches!(fragment.kind, FragmentKind::Line { .. })
            && let FragmentKind::Box { style, .. } = &child.kind
        {
            out.push((
                child.rect.width.to_px(),
                child.rect.height.to_px(),
                style.background_color.b > 0.9,
            ));
        }
        line_boxes(child, out);
    }
}

#[test]
fn draws_the_background_of_the_first_line_behind_its_text() {
    let pages = render(
        "<style>@page { margin: 0 } body { font: 20px/40px Inter; margin: 0 } p { width: 400px }
           p::first-line { background: rgb(0 0 255) }</style>
         <p>First line<br>second line</p>",
    );
    let mut found = Vec::new();
    line_boxes(&pages[0].fragments[0], &mut found);
    // One box, on the first line: as wide as its text and as tall as its font's content area.
    assert_eq!(found.len(), 1, "{found:?}");
    let (width, height, blue) = found[0];
    assert!(blue && width > 50.0 && width < 150.0, "{found:?}");
    assert!(height > 20.0 && height < 30.0, "{found:?}");
}

/// The words of the lorem ipsum.
const LOREM: &str = "Lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt \
                     ut labore et dolore magna aliqua Ut enim ad minim veniam quis nostrud exercitation \
                     ullamco laboris nisi ut aliquip ex ea commodo";

#[test]
fn breaks_pretty_lines_beside_a_floated_first_letter_anew() {
    // The floated letter takes its width off the first two lines, which Chrome's greedy lines
    // count with (Blink's `LineWidths`): the last line, a single short word, takes the word
    // before it along.
    let pages = render(&format!(
        "<style>@page {{ size: 400px 3000px; margin: 20px }} body {{ font: 16px/20px Inter; margin: 0 }}
           p {{ margin: 0; width: 300px; text-wrap: pretty }}
           p::first-letter {{ float: left; font-size: 40px; line-height: 40px }}</style><p>{LOREM}.</p>"
    ));
    let mut lines = Vec::new();
    line_texts(&pages[0].fragments[0], &mut lines);
    assert_eq!(lines.last().map(String::as_str), Some("ea commodo."), "{lines:?}");
}

#[test]
fn breaks_pretty_lines_anew_after_a_page_break() {
    // Three lines of the paragraph go on the first page; its last lines on the next break anew
    // as Chrome breaks them, the last with two words.
    let filler = "<p>Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor \
                  incididunt ut labore et dolore magna aliqua enim.</p>";
    let pages = render(&format!(
        "<style>@page {{ size: 400px 300px; margin: 20px }} body {{ font: 16px/20px Inter; margin: 0 }}
           p {{ margin: 0 0 20px; width: 300px; text-wrap: pretty }}</style>{filler}{filler}
         <p>Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt
           ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation
           ullamco laboris nisi ut aliquip commodo.</p>"
    ));
    let mut lines = Vec::new();
    line_texts(&pages[1].fragments[0], &mut lines);
    assert_eq!(
        lines,
        [
            "dolore magna aliqua. Ut enim",
            "ad minim veniam, quis nostrud",
            "exercitation ullamco laboris nisi ut",
            "aliquip commodo."
        ]
    );
}

#[test]
fn sets_the_first_line_in_made_up_small_capitals() {
    // Inter has no small capitals of its own: the first line makes them up of capitals at 70 %,
    // which splits its words into more pieces than the other lines have. The text goes on after
    // the first line where it ended, every word once.
    let text = "Lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor";
    let pages = render(&format!(
        "<style>@page {{ margin: 0 }} body {{ font: 20px/30px Inter; margin: 0 }} p {{ width: 260px }}
           p::first-line {{ font-variant: small-caps }}</style><p>{text}</p>"
    ));
    let runs = placed_runs(&pages[0].fragments[0]);
    let first = runs.first().map_or(f32::NAN, |run| run.1);
    let small: Vec<_> = runs.iter().filter(|run| (run.3 - 14.0).abs() < 0.01).collect();
    assert!(
        !small.is_empty() && small.iter().all(|run| (run.1 - first).abs() < 0.01),
        "{runs:?}"
    );
    assert!(runs.iter().any(|run| run.1 > first + 1.0), "{runs:?}");
    let words: String = runs.iter().map(|run| run.4.as_str()).collect::<String>();
    let words: Vec<&str> = words.split_whitespace().collect();
    assert_eq!(words.join(" ").to_lowercase(), text.to_lowercase(), "{runs:?}");
}

#[test]
fn puts_no_box_behind_a_first_line_without_a_background() {
    // A `::first-line` that styles the text but has no background (its image layers are
    // `none`) adds no box, which would reach over the spaces that hang at the end of the line.
    let pages = render(
        "<style>@page { margin: 0 } body { font: 20px/40px Inter; margin: 0 } p { width: 400px }
           p::first-line { font-weight: 700; text-transform: uppercase; background-image: none }</style>
         <p>First line<br>second line</p>",
    );
    let mut found = Vec::new();
    line_boxes(&pages[0].fragments[0], &mut found);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn keeps_the_marker_on_a_trimmed_line_of_a_child() {
    let pages = render(
        "<style>@page { margin: 0 } body { font: 20px/2 Inter; margin: 0 }
           ul { margin: 0; padding: 0 0 0 40px } li { text-box: trim-both text }</style>
         <ul><li>X</li></ul><ul><li><div>Y</div></li></ul>",
    );
    let runs = placed_runs(&pages[0].fragments[0]);
    let (x, y) = (baseline(&runs, "X"), baseline(&runs, "Y"));
    // The second item is trimmed as the first, whose line holds its text itself.
    let first = runs.iter().find(|run| run.4 == "X").expect("X").1;
    assert!(x.is_finite() && y.is_finite());
    let height = y - first;
    let (lines_x, lines_y) = (
        runs.iter().filter(|run| (run.1 - x).abs() < 0.01).count(),
        runs.iter().filter(|run| (run.1 - y).abs() < 0.01).count(),
    );
    assert_eq!((lines_x, lines_y), (2, 2), "{runs:?}");
    // Each item is as tall as its trimmed line.
    assert!(
        (height - 20.0 * 0.968_75 - 20.0 * 0.242_187_5).abs() < 2.0,
        "{runs:?}"
    );
}

#[test]
fn widens_tabs_as_spaces_where_lines_are_justified() {
    // The advances of the glyphs of the tab and the space of the first line.
    let advances = |align: &str| {
        fn find(fragment: &Fragment, out: &mut Vec<f32>) {
            if let FragmentKind::Text(run) = &fragment.kind
                && run.text.starts_with('a')
            {
                for glyph in &run.glyphs {
                    if matches!(
                        run.text.as_bytes().get(glyph.cluster.0 as usize),
                        Some(b'\t' | b' ')
                    ) {
                        out.push(glyph.advance);
                    }
                }
            }
            for child in &fragment.children {
                find(child, out);
            }
        }
        let pages = render(&format!(
            "<style>@page {{ margin: 0 }} body {{ font: 16px Inter; margin: 0 }}
               p {{ width: 200px; white-space: pre-wrap; text-align: {align} }}</style>
             <p>a&#9;b c wrapsontothenextlinewhole</p>"
        ));
        let mut out = Vec::new();
        find(&pages[0].fragments[0], &mut out);
        out
    };
    let (left, justified) = (advances("left"), advances("justify"));
    assert!(
        left.len() >= 2 && justified.len() == left.len(),
        "{left:?} {justified:?}"
    );
    // The tab and the space after "b" take the same share of the room left.
    let (tab, space) = (justified[0] - left[0], justified[1] - left[1]);
    assert!(tab > 10.0, "{tab}");
    assert!((space - tab).abs() < 0.01, "{tab} {space}");
}

#[test]
fn takes_the_tabs_before_an_initial_letter_into_its_box() {
    let pages = render(
        "<style>@page { margin: 0 } body { font: 16px/20px Inter; margin: 0 }
           p { white-space: pre; tab-size: 4 } p::first-letter { initial-letter: 2 }</style>
         <p>&#9;Abc\ndef</p>",
    );
    let runs = placed_runs(&pages[0].fragments[0]);
    let run = |text: &str| runs.iter().find(|run| run.4 == text).expect(text).clone();
    let (letter, rest) = (run("A"), run("bc"));
    // The letter stands after the tab, and the lines beside the box that holds both.
    assert!(letter.0 > 10.0, "{runs:?}");
    assert!(rest.0 > letter.0 + 20.0, "{runs:?}");
}

#[test]
fn trims_the_first_line_beside_a_raised_initial_letter() {
    let baseline_of = |letter: &str, text: &str| {
        let pages = render(&format!(
            "<style>@page {{ margin: 0 }} body {{ font: 20px/40px Inter; margin: 0 }}
               p {{ text-box: trim-start text }} {letter}</style><p>Abc def</p>"
        ));
        baseline(&placed_runs(&pages[0].fragments[0]), text)
    };
    // The letter raised by a line does not move the trimmed text down.
    assert_eq!(
        baseline_of("p::first-letter { initial-letter: 2 1 }", "bc"),
        baseline_of("", "Abc")
    );
}

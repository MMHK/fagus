//! Inline layout as Chrome does it: line breaks across the boundaries of inline boxes, ruby
//! across lines, right-to-left tabs, justification and initial letters, emphasis beside an
//! ellipsis.

use super::{find, line_texts, line_tops, render, render_document};
use crate::fragment::{Fragment, FragmentKind};

/// The lines of `content` in a box too narrow for anything but one piece between two break
/// opportunities on each line.
fn narrow_lines(content: &str) -> Vec<String> {
    let pages = render(&format!(
        "<style>body {{ font: 16px Inter; margin: 0 }} div {{ width: 1px }} .n {{ white-space: nowrap }}
           .ib {{ display: inline-block }}</style><div>{content}</div>"
    ));
    let mut lines = Vec::new();
    line_texts(&pages[0].fragments[0], &mut lines);
    lines
}

#[test]
fn breaks_where_the_text_of_the_paragraph_allows() {
    // Between brackets, after punctuation and hyphens, whichever box holds what.
    assert_eq!(narrow_lines("<span>[a]</span><span>[b]</span>"), ["[a]", "[b]"]);
    assert_eq!(narrow_lines("a,<span>(b</span>"), ["a,", "(b"]);
    assert_eq!(narrow_lines("<span>a</span>-1"), ["a-", "1"]);
    assert_eq!(narrow_lines("1<span>-</span>2"), ["1-", "2"]);
    // Not inside a word that goes on in the next box.
    assert_eq!(narrow_lines("a<span>b</span>c"), ["abc"]);
    assert_eq!(narrow_lines("<span>a</span><span>.b</span>"), ["a.b"]);
}

#[test]
fn breaks_at_the_end_of_text_that_does_not_wrap() {
    // After a box that does not wrap, where the text allows, but not after a space in it.
    assert_eq!(narrow_lines("<span class=n>a-</span>b"), ["a-", "b"]);
    assert_eq!(narrow_lines("<span class=n>[a]</span>[b]"), ["[a]", "[b]"]);
    assert_eq!(narrow_lines("<span class=n>a b</span>c"), ["a bc"]);
    // A break inside it goes on to after it.
    assert_eq!(
        narrow_lines("<span class=n>x <span style='white-space: normal'>a-</span></span>b"),
        ["x a-", "b"]
    );
}

#[test]
fn breaks_around_atomic_inlines_by_the_box_they_are_in() {
    // Its own `white-space` does not matter, that of the box it is in does.
    assert_eq!(narrow_lines("a<span class='ib n'>X</span>b"), ["a", "X", "b"]);
    assert_eq!(
        narrow_lines("<span class=n>a<span class=ib style='white-space: normal'>X</span>b</span>"),
        ["aXb"]
    );
    assert_eq!(
        narrow_lines("<span class=n>a</span><span class=ib>X</span>b"),
        ["a", "X", "b"]
    );
}

#[test]
fn breaks_at_word_break_opportunities() {
    assert_eq!(narrow_lines("abc<wbr>def"), ["abc\u{200B}", "def"]);
    // Even where lines do not wrap otherwise.
    assert_eq!(
        narrow_lines("<span class=n>abc<wbr>def</span>"),
        ["abc\u{200B}", "def"]
    );
}

#[test]
fn breaks_by_word_break_at_the_end_of_a_box() {
    assert_eq!(
        narrow_lines("<span style='word-break: break-all'>ab</span>cd"),
        ["a", "b", "cd"]
    );
    assert_eq!(
        narrow_lines("<span>ab</span><span style='word-break: break-all'>cd</span>"),
        ["abc", "d"]
    );
    assert_eq!(
        narrow_lines("<span style='word-break: keep-all'>日本</span>語"),
        ["日本語"]
    );
}

/// The text runs of a fragment with their x, baseline and width on the page.
fn placed_runs(fragment: &Fragment) -> Vec<(f32, f32, f32, String)> {
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

#[test]
fn breaks_ruby_like_the_text_of_its_base() {
    // No break around a column inside a word, as there is none inside the word.
    assert_eq!(narrow_lines("ab<ruby>cd<rt>x</rt></ruby>ef gh").len(), 2);
    // A long ruby breaks between the words of its base, its annotation by its share.
    let pages = render(
        "<style>body { font: 20px Inter; margin: 0 } p { width: 120px }</style>
         <p>The <ruby>quick brown fox jumps<rt>an annotation</rt></ruby> end</p>",
    );
    let runs = placed_runs(&pages[0].fragments[0]);
    let texts: Vec<&str> = runs.iter().map(|run| run.3.trim()).collect();
    assert_eq!(
        texts,
        ["The", "an", "quick", "annotation", "brown fox", "jumps", "end"]
    );
    // Each part's annotation sits over it, and the last line, without one, is closer, as in
    // Chrome.
    let baselines: Vec<f32> = runs.iter().map(|run| run.1).collect();
    assert_eq!(baselines[2] - baselines[1], 19.0);
    assert_eq!(baselines[4] - baselines[3], 19.0);
    assert_eq!(
        (baselines[4] - baselines[2], baselines[5] - baselines[4]),
        (31.0, 24.0)
    );
}

#[test]
fn puts_the_annotations_of_a_line_on_one_baseline() {
    let pages = render(
        "<style>body { font: 20px Inter; margin: 0 }</style>
         <p><ruby>a<rt>x</rt></ruby><span style='font-size: 40px'><ruby>b<rt>y</rt></ruby></span>\
         <ruby style='ruby-position: under'>c<rt>z</rt></ruby></p>",
    );
    let runs = placed_runs(&pages[0].fragments[0]);
    let baseline = |text: &str| runs.iter().find(|run| run.3 == text).map(|run| run.1);
    // Over the larger base, both.
    assert_eq!(baseline("x"), baseline("y"));
    assert!(baseline("x") < baseline("a"));
    assert!(baseline("z") > baseline("c"));
}

#[test]
fn sets_tabs_and_justifies_right_to_left() {
    // Text between tabs is ordered on its own, the tabs going from the right: "ab" at the
    // right edge, a tab to the stop 8 spaces from there, "cd" left of it, as in Chrome.
    let pages = render(
        "<style>@page { margin: 0 } body { font: 20px Inter; margin: 0 }
           p { width: 300px; white-space: pre; direction: rtl }</style><p>ab\tcd</p>",
    );
    let runs = placed_runs(&pages[0].fragments[0]);
    assert_eq!(runs.len(), 1);
    let (x, _, width, text) = &runs[0];
    assert_eq!(text, "cd\tab");
    assert!(
        (x + width - 300.0).abs() < 0.01 && (x - 231.33).abs() < 0.02,
        "{runs:?}"
    );
    // With `inter-character`, the glyph at the right ends at the right edge.
    let pages = render(
        "<style>@page { margin: 0 } body { font: 20px Inter; margin: 0 }
           p { width: 200px; direction: rtl; unicode-bidi: bidi-override;
           text-align: justify; text-justify: inter-character }</style><p>abc def ghi jkl mno pqr</p>",
    );
    let runs = placed_runs(&pages[0].fragments[0]);
    let right = runs
        .iter()
        .filter(|run| !run.3.trim().is_empty())
        .map(|run| run.0 + run.2)
        .fold(0.0, f32::max);
    assert!((right - 200.0).abs() < 0.01, "{runs:?}");
}

#[test]
fn puts_initial_letters_on_the_right_in_right_to_left_text() {
    let pages = render(
        "<style>@page { margin: 0 } body { font: 20px Inter; margin: 0 } p { width: 200px }
           p::first-letter { initial-letter: 2 }</style>
         <p dir=rtl>Letters to the right of a paragraph</p>",
    );
    let runs = placed_runs(&pages[0].fragments[0]);
    let letter = runs.iter().find(|run| run.3 == "L").expect("the letter");
    assert!(letter.0 > 150.0, "{runs:?}");
}

#[test]
fn makes_no_room_for_emphasis_over_an_ellipsis() {
    let pages = render(
        "<style>@page { margin: 0 } body { font: 20px/20px Inter; margin: 0 } p { margin: 0; width: 60px; overflow: hidden;
           white-space: nowrap; text-overflow: ellipsis; text-emphasis: dot } span { text-emphasis: none }</style>
         <p><span>A sentence too long</span></p>",
    );
    let mut lines = Vec::new();
    line_tops(&pages[0].fragments[0], 0.0, &mut lines);
    assert_eq!(lines[0].0, 0.0);
}

/// The widths of the text runs of the lines of `content` in a box `width` wide.
fn run_widths(content: &str, width: &str) -> Vec<f32> {
    let pages = render(&format!(
        "<style>body {{ font: 16px Inter; margin: 0 }} div {{ width: {width} }}</style><div>{content}</div>"
    ));
    placed_runs(&pages[0].fragments[0])
        .into_iter()
        .map(|(_, _, width, _)| width)
        .collect()
}

#[test]
fn puts_space_between_ideographs_and_latin_letters() {
    // An eighth of an em (here the font size: the font has no ideographs), after the
    // character before.
    let plain: f32 = run_widths("漢a1字", "auto").iter().sum();
    let spaced: f32 = run_widths("<span style='text-autospace: normal'>漢a1字</span>", "auto")
        .iter()
        .sum();
    assert!((spaced - plain - 4.0).abs() < 0.01, "{plain} {spaced}");
    // Not at the end of a line.
    let lines = run_widths("<span style='text-autospace: normal'>漢a</span>", "1px");
    assert_eq!(lines, run_widths("漢a", "1px"));
}

/// How far the annotation of a ruby column after `before` reaches over that text, in a
/// paragraph in `font`.
fn ruby_overhang(before: &str, font: &str) -> f32 {
    let pages = render(&format!(
        "<style>@page {{ margin: 0 }} body {{ font: {font}; margin: 0 }}</style>
         <p>{before}<ruby>x<rt>wwwwwwww</rt></ruby>abc</p>"
    ));
    let runs = placed_runs(&pages[0].fragments[0]);
    let find = |text: &str| runs.iter().find(|run| run.3 == text).expect("a run");
    let text = find(before);
    text.0 + text.2 - find("wwwwwwww").0
}

#[test]
fn reaches_ruby_annotations_over_the_text_beside_them_as_chrome_does() {
    // Half the font size of the annotation in whole pixels, rounded down: 8.5px gives 4.
    assert!((ruby_overhang("abc", "17px Inter") - 4.0).abs() < 0.05);
    assert!((ruby_overhang("abc", "20px Inter") - 5.0).abs() < 0.05);
    // No more than half the text before.
    let pages = render("<style>body { font: 20px Inter }</style><p>i</p>");
    let width = placed_runs(&pages[0].fragments[0])[0].2;
    assert!((ruby_overhang("i", "20px Inter") - width / 2.0).abs() < 0.05);
}

#[test]
fn spaces_ruby_bases_from_the_text_beside_them_but_not_annotations() {
    let pages = render(
        "<style>@page { margin: 0 } body { font: 16px Inter; margin: 0; text-autospace: normal }
           rt { font-size: 16px }</style>
         <p>a<ruby>漢<rt>漢b</rt></ruby>a</p><p>漢b</p>",
    );
    let runs = placed_runs(&pages[0].fragments[0]);
    let [before, annotation, base, after, plain] = &runs[..] else {
        panic!("{runs:?}");
    };
    assert_eq!(
        (before.3.as_str(), base.3.as_str(), after.3.as_str()),
        ("a", "漢", "a")
    );
    // An eighth of the font size (the font has no ideographs) before the base, in the ruby
    // column, and after it, before the text that follows.
    assert!((base.0 - before.0 - before.2 - 2.0).abs() < 0.01, "{runs:?}");
    assert!((after.2 - before.2 - 2.0).abs() < 0.01, "{runs:?}");
    // The annotation has none, unlike the same text in the paragraph.
    assert!((plain.2 - annotation.2 - 2.0).abs() < 0.01, "{runs:?}");
}

#[test]
fn keeps_the_spacing_of_words_broken_by_overflow_wrap() {
    let style = "<style>@page { margin: 0 } body { font: 16px Inter; margin: 0; text-autospace: normal }
       div { word-break: keep-all; overflow-wrap: anywhere }</style>";
    let pages = render(&format!("{style}<div style='width: max-content'>漢aa</div>"));
    let width: f32 = placed_runs(&pages[0].fragments[0]).iter().map(|run| run.2).sum();
    let pages = render(&format!(
        "{style}<div style='width: {}px'>漢aa漢aa</div>",
        width + 0.5
    ));
    let mut lines = Vec::new();
    line_texts(&pages[0].fragments[0], &mut lines);
    assert_eq!(lines, ["漢aa", "漢aa"]);
    // The second line keeps the space between the ideograph and the letter.
    let runs = placed_runs(&pages[0].fragments[0]);
    let second: f32 = runs.iter().filter(|run| run.1 > runs[0].1).map(|run| run.2).sum();
    assert!((second - width).abs() < 0.01, "{runs:?}");
}

#[test]
fn lets_ruby_annotations_reach_into_the_room_under_the_block_before() {
    // The annotation of the second paragraph reaches into the room the first leaves under its
    // text, even without annotations of its own.
    let second_top = |line_height: &str| {
        let pages = render(&format!(
            "<style>@page {{ margin: 0 }} body {{ font: 30px/40px Inter; margin: 0 }} p {{ margin: 0 }}</style>
             <p style='line-height: {line_height}'>abc</p><p>a<ruby>b<rt>c</rt></ruby></p>"
        ));
        let mut lines = Vec::new();
        line_tops(&pages[0].fragments[0], 0.0, &mut lines);
        lines[1].0
    };
    let (tight, loose) = (second_top("30px"), second_top("40px"));
    // The taller first line leaves room under its text, and the second moves down less.
    assert!(loose < tight + 10.0 - 1.0, "{tight} {loose}");
}

#[test]
fn stacks_the_annotations_of_nested_ruby() {
    let pages = render(
        "<style>body { font: 20px Inter; margin: 0 }</style>
         <p><ruby><ruby>a<rt>x</rt></ruby><rt>y</rt></ruby><ruby>b<rt>z</rt></ruby></p>
         <p><ruby style='ruby-position: under'><ruby style='ruby-position: over'>c<rt>v</rt></ruby><rt>w</rt></ruby></p>",
    );
    let runs = placed_runs(&pages[0].fragments[0]);
    let baseline = |text: &str| {
        runs.iter()
            .find(|run| run.3 == text)
            .map(|run| run.1)
            .expect(text)
    };
    // The annotation of the outer column goes over that of the inner one, which shares its
    // baseline with the column beside it.
    assert_eq!(baseline("x"), baseline("z"));
    assert!(baseline("y") < baseline("x") - 5.0, "{runs:?}");
    assert!(baseline("x") < baseline("a"));
    // Double-sided: one over the base, one under it.
    assert!(
        baseline("v") < baseline("c") && baseline("w") > baseline("c"),
        "{runs:?}"
    );
}

#[test]
fn leaves_less_room_for_annotations_after_a_block_with_top_padding() {
    // Chrome counts the top padding of the block before twice, so the annotation of the next
    // block finds that much less room and its line moves down by it again.
    let second_top = |padding: &str| {
        let pages = render(&format!(
            "<style>@page {{ margin: 0 }} body {{ font: 30px/40px Inter; margin: 0 }} p {{ margin: 0 }}</style>
             <p style='padding-top: {padding}'>abc</p><p>a<ruby>b<rt>c</rt></ruby></p>"
        ));
        let mut lines = Vec::new();
        line_tops(&pages[0].fragments[0], 0.0, &mut lines);
        lines[1].0
    };
    let moved = second_top("2px") - second_top("0px");
    assert!((moved - 4.0).abs() < 0.01, "{moved}");
}

#[test]
fn fits_text_up_to_a_layout_unit_wider_than_the_line() {
    // The text is 438.953 pixels wide in layout units, as Chrome on Linux measures Inter at
    // 10pt (set at 853/64 pixels): it fits on a line 1/64 pixel narrower, 438.9375 pixels, but
    // not on one of 438.921875 (on macOS, where it is 439.011 pixels wide, on 439 but not on
    // 438.984375).
    let lines = |width: &str| {
        let pages = render(&format!(
            "<style>body {{ font: 10pt Inter; margin: 0 }} p {{ margin: 0; width: {width} }}</style>
             <p>customers logistics. Service supply by and vehicles committee new is</p>"
        ));
        let mut lines = Vec::new();
        line_texts(&pages[0].fragments[0], &mut lines);
        lines.len()
    };
    assert_eq!(lines("438.9375px"), 1);
    assert_eq!(lines("438.921875px"), 2);
}

#[test]
fn breaks_pretty_and_balanced_paragraphs_as_chrome_does() {
    // Lines and their texts as Chrome breaks them (Blink's `ScoreLineBreaker`).
    let lines = |css: &str, text: &str| {
        let pages = render(&format!(
            "<html lang=de><style>@page {{ margin: 0 }} body {{ font: 16px/20px Inter; margin: 0 }}
               p {{ width: 300px; margin: 0; {css} }}</style><p>{text}</p>"
        ));
        let mut found = Vec::new();
        line_texts(&pages[0].fragments[0], &mut found);
        found
            .iter()
            .map(|line| line.trim().replace('\u{AD}', "-"))
            .collect::<Vec<_>>()
    };
    let broken = "Sehr geehrte Damen und Herren wir erlauben<br>uns Ihnen die folgenden Leistungen in \
                  Rechnung zu stellen Ende";
    // Each part of a pretty paragraph that a forced break ends is a paragraph of its own.
    assert_eq!(
        lines("text-wrap: pretty", broken),
        [
            "Sehr geehrte Damen und Herren",
            "wir erlauben",
            "uns Ihnen die folgenden Leistungen in",
            "Rechnung zu stellen Ende"
        ]
    );
    // Only the first part of a balanced one is balanced.
    assert_eq!(
        lines("text-wrap: balance", broken),
        [
            "Sehr geehrte Damen",
            "und Herren wir erlauben",
            "uns Ihnen die folgenden Leistungen in",
            "Rechnung zu stellen Ende"
        ]
    );
    // The last line is a single word but too long to be worth breaking anew; the last word of
    // the paragraph is not hyphenated where it does not start its line.
    assert_eq!(
        lines(
            "text-wrap: pretty; text-align: justify; hyphens: auto",
            "Sehr geehrte Damen und Herren wir erlauben uns Ihnen die folgenden Leistungen in \
             Rechnung zu stellen und Gesamtbetrag"
        ),
        [
            "Sehr geehrte Damen und Herren wir er-",
            "lauben uns Ihnen die folgenden Leis-",
            "tungen in Rechnung zu stellen und",
            "Gesamtbetrag"
        ]
    );
}

#[test]
fn counts_the_trailing_spaces_of_preformatted_text_in_its_width() {
    // Spaces at the end of a line of `white-space: pre` do not hang: Chrome makes a button
    // labelled " " as wide as a space, an inline block of "x " wider than one of "x".
    let (document, pages) = render_document(
        "<!DOCTYPE html><style>body { margin: 0; font: 16px/20px Inter }
           span { display: inline-block; white-space: pre }</style>
         <p><span class=bare>x</span></p><p><span class=space>x </span></p>
         <p><span class=wrap style='white-space: pre-wrap'>x </span></p>",
    );
    let width = |class: &str| {
        find(&pages[0], &document, class)
            .expect(class)
            .2
            .rect
            .width
            .to_px()
    };
    assert!(width("space") > width("bare") + 3.0);
    // Preserved spaces that wrap hang at the end of the paragraph only where they overflow:
    // they count too, as in Chrome.
    assert_eq!(width("wrap"), width("space"));
}

#[test]
fn starts_each_inline_item_where_the_one_before_ends_in_layout_units() {
    // Chrome rounds the width of each item (the text of a box) up to 1/64 pixel: what follows
    // starts there, not where the glyphs end.
    let pages = render(
        "<style>body { font: 14px Inter; margin: 0 }</style>
         <div>a<span>H</span><span>x</span><span>g</span>y</div>",
    );
    let runs = placed_runs(&pages[0].fragments[0]);
    assert_eq!(runs.len(), 5, "{runs:?}");
    for pair in runs.windows(2) {
        let ((x, _, width, _), (next, _, _, _)) = (&pair[0], &pair[1]);
        assert!((x + width - next).abs() < 1e-4, "{runs:?}");
        assert_eq!((next * 64.0).fract(), 0.0, "{runs:?}");
    }
}

#[test]
fn fits_a_line_by_the_advances_chrome_measures() {
    // Core Text's advances reach HarfBuzz cut to 1/65536 pixel: this line of 10pt Inter is
    // 701.0153 pixels wide in Chrome, which fits 701 pixels (and one layout unit), not 700.9.
    let text = "Revenue on capital notice decreased income safety report across total. Energy the \
                product and to notice margin previous compared company process year contract \
                overall in protection. Growth update overall revenue previous was during";
    let lines = |width: &str| {
        let pages = render(&format!(
            "<style>body {{ font: 10pt/1.45 Inter; margin: 0 }} p {{ width: {width}; margin: 0 }}</style>
             <p>{text}</p>"
        ));
        let mut lines = Vec::new();
        line_texts(&pages[0].fragments[0], &mut lines);
        lines
    };
    assert!(
        lines("701px")[1].ends_with("overall revenue"),
        "{:?}",
        lines("701px")
    );
    assert!(
        lines("700.9px")[1].ends_with("update overall"),
        "{:?}",
        lines("700.9px")
    );
}

#[test]
fn makes_the_max_content_width_of_the_items_rounded_up() {
    // Each item counts rounded up to layout units: words in boxes of their own stay on one
    // line in a box as wide as its max-content width.
    let words: String = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]
        .map(|word| format!("<span>{word}</span> "))
        .concat();
    let pages = render(&format!(
        "<style>body {{ font: 13.7px Inter; margin: 0 }}</style>
         <div style='float: left'>{words}</div>"
    ));
    let mut lines = Vec::new();
    line_texts(&pages[0].fragments[0], &mut lines);
    assert!(
        lines.iter().any(|line| line == "a b c d e f g h i j"),
        "{lines:?}"
    );
}

#[test]
fn splits_leading_as_blink_does() {
    // Half of it in layout units, cut toward zero, then floored to whole pixels.
    assert_eq!(crate::inline::half_leading(2.0), 1.0);
    assert_eq!(crate::inline::half_leading(3.0), 1.0);
    assert_eq!(crate::inline::half_leading(-1.0 / 64.0), 0.0);
    assert_eq!(crate::inline::half_leading(-3.0 / 64.0), -1.0);
    assert_eq!(crate::inline::half_leading(-2.0), -1.0);
    assert_eq!(crate::inline::half_leading(-3.0), -2.0);
    // 10pt Inter is 13 + 3 pixels high; a line of 15.99 pixels (1023/64) puts its baseline
    // 13 pixels down, as one of 16.
    let baseline = |line_height: &str| {
        let pages = render(&format!(
            "<style>body {{ font: 10pt/{line_height} Inter; margin: 0 }}</style><div>x</div>"
        ));
        placed_runs(&pages[0].fragments[0])[0].1
    };
    assert_eq!(baseline("15.99px"), baseline("16px"));
    assert_eq!(baseline("15.97px"), baseline("16px") - 1.0);
}

#[test]
fn gives_text_fields_a_normal_line_no_lower_than_the_font() {
    // Blink's inner editor takes `line-height: normal` where the field's is no more than its
    // font size, or where the field has a height larger than it.
    let (document, pages) = render_document(
        "<!DOCTYPE html><style>body { margin: 0; font: 16px Inter } input { display: block }</style>
         <input class=one value=x style='line-height: 1'>
         <input class=normal value=x>
         <input class=taller value=x style='line-height: 17px'>
         <input class=high value=x style='line-height: 17px; height: 30px'>",
    );
    let field = |class: &str| {
        find(&pages[0], &document, class)
            .expect(class)
            .2
            .rect
            .height
            .to_px()
    };
    assert_eq!(field("one"), field("normal"));
    assert_eq!(field("taller"), 17.0 + 2.0 + 4.0);
    // The field's own height, centered in it a normal line.
    assert_eq!(field("high"), 30.0 + 2.0 + 4.0);
}

#[test]
fn measures_text_with_the_advances_harfbuzz_has_in_chrome() {
    // Blink passes each advance through HarfBuzz in 16.16 fixed point, cut towards zero: the
    // line is a little narrower than the exact sum of its advances and just fits in 701px
    // (where the exact sum, 1/64 px more than the room and a bit, would not), as in Chrome.
    let text = "margin previous compared company process year contract overall in protection. \
                Growth update overall revenue previous";
    let pages = render(&format!(
        "<style>body {{ font: 10pt/1.45 Inter; margin: 0 }} p {{ width: 701px; margin: 0 }}</style>
         <p>{text}</p>"
    ));
    let mut lines = Vec::new();
    line_texts(&pages[0].fragments[0], &mut lines);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].ends_with("revenue"), "{lines:?}");
}

#[test]
fn starts_absolute_boxes_that_were_blocks_at_the_start_of_the_line() {
    // As Chrome places them (Blink's `PlaceOutOfFlowObjects`): a box that was a block starts at
    // the start of its line whatever its `text-align`, below the line where it follows content
    // (KaTeX's stretchy SVGs in a centred formula); one that was inline stays where it is.
    let (document, pages) = render_document(
        "<style>@page { size: 400px 400px; margin: 0 } body { margin: 0; font: 16px/20px Inter }
           div { position: relative; width: 200px; height: 40px; text-align: center }
           .ib { display: inline-block; position: relative; width: 100px; height: 20px }
           .a { position: absolute; width: 10px; height: 10px }</style>
         <div><span class=ib><svg class='a one' style='display: block'></svg></span></div>
         <div><span class='a two' style='display: block'></span></div>
         <div>xx<span class='a three' style='display: block'></span>yy</div>
         <div><span class='a four'></span></div>",
    );
    let page = &pages[0];
    let at = |class: &str| {
        let (x, y, _) = find(page, &document, class).expect(class);
        (x, y)
    };
    assert_eq!(at("one"), (50.0, 0.0), "at the start of the inline block's line");
    assert_eq!(at("two"), (0.0, 40.0));
    assert_eq!(at("three"), (0.0, 100.0), "below the line, at its start");
    assert_eq!(
        at("four"),
        (100.0, 120.0),
        "in the middle, where an inline box goes"
    );
}

#[test]
fn sizes_atomic_inlines_with_the_margins_of_inline_boxes_in_layout_units() {
    // Blink cuts a margin of 1.33668 pixels to 85/64 (`LayoutUnit(float)`): an inline block that
    // holds only it is that wide, and what follows starts there.
    let (document, pages) = render_document(
        "<style>@page { size: 400px 400px; margin: 0 } body { margin: 0; font: 16px/20px Inter }
           .ib { display: inline-block }</style>
         <div><span class=ib><span style='margin-right: 1.33668px'></span></span><span class=after></span></div>",
    );
    let (x, _, _) = find(&pages[0], &document, "after").expect("after");
    assert_eq!(x, 85.0 / 64.0);
}

#[test]
fn drops_the_glyphs_of_spaces_that_end_a_line() {
    // Chrome removes collapsible spaces at the end of a line (`RemoveTrailingCollapsibleSpace`),
    // also before boxes that open there; they keep their room but are not drawn. Preserved
    // spaces stay.
    fn runs(fragment: &Fragment, out: &mut Vec<(String, usize)>) {
        if let FragmentKind::Text(run) = &fragment.kind {
            out.push((run.text.clone(), run.glyphs.len()));
        }
        for child in &fragment.children {
            runs(child, out);
        }
    }
    let pages = render(
        "<style>@page { size: 400px 400px; margin: 0 } body { margin: 0; font: 16px/20px Inter }
           div { width: 50px } .a { position: absolute }</style>
         <div>aaaa bbbb</div><div>aaaa <span><span class=a></span>bbbb</span></div>
         <div style='white-space: pre-wrap'>cccc dddd</div>",
    );
    let mut found = Vec::new();
    runs(&pages[0].fragments[0], &mut found);
    let glyphs = |text: &str| -> Vec<usize> {
        found
            .iter()
            .filter(|(run, _)| run.trim_end() == text)
            .map(|(_, glyphs)| *glyphs)
            .collect()
    };
    assert_eq!(glyphs("aaaa"), [4, 4], "{found:?}");
    assert_eq!(glyphs("cccc"), [5], "{found:?}");
}

#[test]
fn keeps_the_optical_size_of_the_text_for_made_up_small_capitals() {
    // Inter's variable font has no small capitals: Chrome sets them as capitals at 70 % of
    // 37.33 pixels, 26, in the instance of the full size (`opsz` 32, its largest), and their
    // emphasis marks at half the full size.
    use std::sync::Arc;

    use fagus_text::{Descriptors, Face, FontDb, unpack};
    fn small(fragment: &Fragment) -> Option<&crate::TextRun> {
        match &fragment.kind {
            FragmentKind::Text(run) if run.text == "b" => Some(run),
            _ => fragment.children.iter().find_map(small),
        }
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/webfonts/Inter-Variable-Latin.woff");
    let data = Arc::new(unpack(std::fs::read(path).expect("font")).expect("unpacked"));
    let rule = Descriptors {
        family: "Variable".to_owned(),
        ..Descriptors::default()
    };
    let mut fonts = FontDb::new();
    fonts.add_document_face(Arc::new(Face::from_rule(data, 0, &rule).expect("face")));
    let (_, pages) = super::render_document_in(
        "<style>p { font: 37.33px Variable; font-variant: small-caps; text-emphasis: 'x' }</style><p>Ab</p>",
        &fonts,
        |_| crate::Images::default(),
    );
    let run = pages[0]
        .fragments
        .iter()
        .find_map(small)
        .expect("the small capital");
    assert_eq!(run.size, 26.0);
    let optical = crate::run_variations(&fonts, run)
        .into_iter()
        .find(|(tag, _)| tag == b"opsz");
    assert_eq!(optical, Some((*b"opsz", 32.0)));
    let marks = crate::emphasis_marks(&fonts, run, None).expect("marks");
    assert_eq!(marks.size, 19.0);
}

#[test]
fn moves_a_line_below_a_float_that_starts_it_where_its_first_box_does_not_fit() {
    // The float stays at the top of the line; the box as wide as the line goes below it, with
    // or without white space between them (WPT css/CSS2/normal-flow/inline-replaced-width-012).
    for gap in ["", " "] {
        let (document, pages) = render_document(&format!(
            "<style>@page {{ size: 800px 600px; margin: 0 }} body {{ margin: 0 }}
             p {{ width: 600px; margin: 0 }}
             .f {{ float: left; width: 100px; height: 100px }}
             .w {{ display: inline-block; vertical-align: bottom; width: 100%; height: 100px }}</style>
             <p><span class=f></span>{gap}<span class=w></span></p>"
        ));
        let (fx, fy, _) = find(&pages[0], &document, "f").expect("float");
        let (wx, wy, _) = find(&pages[0], &document, "w").expect("wide box");
        assert_eq!((fx, fy), (0.0, 0.0), "{gap:?}");
        assert_eq!((wx, wy), (0.0, 100.0), "{gap:?}");
    }
    // A box that fits beside the float stays on the line.
    let (document, pages) = render_document(
        "<style>@page { size: 800px 600px; margin: 0 } body { margin: 0 }
         p { width: 600px; margin: 0 }
         .f { float: left; width: 100px; height: 100px }
         .w { display: inline-block; vertical-align: top; width: 500px; height: 100px }</style>
         <p><span class=f></span><span class=w></span></p>",
    );
    let (wx, wy, _) = find(&pages[0], &document, "w").expect("box");
    assert_eq!((wx, wy), (100.0, 0.0));
}

#[test]
fn places_a_float_that_starts_an_indented_line_without_the_indent() {
    // As wide as the line, it fits at its top: the text indent goes to the text below it
    // (WPT css/css-text/text-indent/below-float).
    let (document, pages) = render_document(
        "<style>@page { size: 800px 600px; margin: 0 } body { margin: 0 }
         p { width: 100px; margin: 0; text-indent: 50px; font: 20px/20px Inter }
         .f { float: left; width: 100px; height: 50px }</style>
         <p><span> <span class=f></span> <span class=t>x</span></span></p>",
    );
    let (fx, fy, _) = find(&pages[0], &document, "f").expect("float");
    let (tx, ty, _) = find(&pages[0], &document, "t").expect("text");
    assert_eq!((fx, fy), (0.0, 0.0));
    // On the line below the float (whose top is 50; the text's content area reaches above it).
    assert_eq!(tx, 50.0);
    assert!((45.0..50.0).contains(&ty), "{ty}");
}

#[test]
fn sizes_a_float_around_floats_that_clear_each_other_by_the_widest() {
    // Floats side by side add up; one that clears those before starts a row of its own, as in
    // Blink's `FloatsMaxSize`.
    let (document, pages) = render_document(
        "<style>@page { size: 800px 600px; margin: 0 } body { margin: 0 }
         .c { float: left } .f { float: left; height: 10px }
         .a { width: 80px } .b { width: 30px; clear: left } .d { width: 50px }</style>
         <div class='c one'><span class='f a'></span><span class='f b'></span><span class='f d'></span></div>",
    );
    let (_, _, container) = find(&pages[0], &document, "one").expect("container");
    // 80, then 30 and 50 beside each other.
    assert!(
        (container.rect.width.to_px() - 80.0).abs() < 0.01,
        "{:?}",
        container.rect
    );
}

#[test]
fn shows_images_of_generated_content() {
    use crate::image::{Image, ImageFormat, Images};
    use std::sync::Arc;
    fn images(fragment: &Fragment, out: &mut Vec<(f32, f32)>) {
        if let FragmentKind::Image(_) = &fragment.kind {
            out.push((fragment.rect.width.to_px(), fragment.rect.height.to_px()));
        }
        for child in &fragment.children {
            images(child, out);
        }
    }
    // `content: "x" url(icon.png) "y"`: the image between the texts, at its natural size, on
    // the line (CSS Generated Content 3, §1.1); an image that does not load shows nothing.
    let (_, pages) = super::render_document_with(
        "<style>@page { size: 400px 300px; margin: 0 } body { margin: 0; font: 16px/20px Inter }
         p::before { content: 'x' url(icon.png) 'y' url(missing.png) }</style><p>Z</p>",
        |_| {
            let mut images = Images::default();
            images.insert(
                "icon.png",
                Arc::new(Image {
                    data: Arc::new(Vec::new()),
                    format: ImageFormat::Png,
                    width: 30.0,
                    height: 12.0,
                    natural: (true, true),
                    ratio: Some(2.5),
                    inline: false,
                    orientation: 1,
                    filters: Arc::from([]),
                    pad: 0,
                }),
            );
            images
        },
    );
    let mut found = Vec::new();
    for fragment in &pages[0].fragments {
        images(fragment, &mut found);
    }
    assert_eq!(found, [(30.0, 12.0)]);
    let mut lines = Vec::new();
    line_texts(&pages[0].fragments[0], &mut lines);
    assert_eq!(lines, ["xyZ"]);
}

#[test]
fn keeps_lines_with_tall_boxes_clear_of_floats_lower_down() {
    // The second box would start beside the left float, but its 50 pixels reach down to the
    // right float, which leaves no room: it goes below both (WPT
    // css/CSS2/floats/floats-wrap-top-below-inline-002l).
    let (document, pages) = render_document(
        "<!doctype html><style>@page { size: 600px 800px; margin: 0 } body { margin: 0; font: 16px/20px Inter }
         .c { width: 400px } .l { float: left; width: 150px; height: 75px }
         .r { float: right; width: 300px; height: 75px }
         .b { display: inline-block; vertical-align: top; width: 200px; height: 50px }</style>
         <div class=c><div class=l></div><div class=r></div><span class='b one'></span> <span class='b two'></span></div>",
    );
    let at = |class: &str| {
        let (x, y, _) = find(&pages[0], &document, class).expect(class);
        (x, y)
    };
    assert_eq!(at("one"), (150.0, 0.0));
    assert_eq!(at("two"), (0.0, 150.0));
}

/// The glyphs of the text of `fragment` and its descendants, sorted.
fn glyph_ids(fragment: &Fragment, out: &mut Vec<u32>) {
    if let FragmentKind::Text(run) = &fragment.kind {
        out.extend(run.glyphs.iter().map(|glyph| glyph.id));
    }
    for child in &fragment.children {
        glyph_ids(child, out);
    }
}

/// The sorted glyphs of the lines of `body` in a system font of macOS (`file`, `family`);
/// `None` where the font is missing.
fn arabic_glyphs(file: &str, family: &str, body: &str) -> Option<Vec<u32>> {
    let mut fonts = fagus_text::FontDb::new();
    fonts.add_font_file(std::path::Path::new(file)).ok()?;
    let html = format!(
        "<style>@page {{ margin: 0 }} body {{ font: 20px '{family}'; margin: 0 }}
           p {{ direction: rtl }} .m {{ margin: 0 4px; border: 1px solid }} .i {{ unicode-bidi: isolate }}
         </style><p>{body}</p>"
    );
    let (_, pages) = super::render_document_in(&html, &fonts, |_| crate::Images::default());
    let mut ids = Vec::new();
    glyph_ids(&pages[0].fragments[0], &mut ids);
    ids.sort_unstable();
    Some(ids)
}

#[test]
fn joins_arabic_letters_across_inline_boxes() {
    // An OpenType face, joined by HarfBuzz's context; and Geeza Pro, whose `morx` table only
    // joins letters shaped at once, as Chrome shapes adjacent items of the same font.
    for (file, family) in [
        ("/System/Library/Fonts/SFArabic.ttf", "SF Arabic"),
        ("/System/Library/Fonts/GeezaPro.ttc", "Geeza Pro"),
    ] {
        let Some(whole) = arabic_glyphs(file, family, "ععع") else {
            continue;
        };
        assert_eq!(
            arabic_glyphs(file, family, "ع<span>ع</span>ع"),
            Some(whole.clone()),
            "{family}"
        );
        assert_eq!(
            arabic_glyphs(file, family, "ع<b style='color: red'>ع</b>ع").map(|ids| ids.len()),
            Some(3),
            "{family}"
        );
        // The lam-alef ligature across the edge of a box goes to the box of the lam.
        assert_eq!(
            arabic_glyphs(file, family, "ع<span>ل</span>ا"),
            arabic_glyphs(file, family, "علا"),
            "{family}"
        );
        // Not across the edge of an isolated box.
        assert_ne!(
            arabic_glyphs(file, family, "ع<span class=i>ع</span>ع"),
            Some(whole.clone())
        );
        if family == "SF Arabic" {
            // A margin and a border keep the forms, which the context gives.
            assert_eq!(
                arabic_glyphs(file, family, "ع<span class=m>ع</span>ع"),
                Some(whole)
            );
        }
    }
}

#[test]
fn hangs_preserved_spaces_only_where_they_overflow_at_the_end_of_a_paragraph() {
    // `pre-wrap`: the spaces before a forced break or at the end take room for the alignment,
    // those where a line wraps hang; an inline block is as wide as its spaces too.
    let pages = render(
        "<style>@page { margin: 0 } body { font: 20px/20px Inter; margin: 0 }
           p { margin: 0; width: 200px; white-space: pre-wrap; text-align: right }
           .ib { display: inline-block; text-align: left }</style>
         <p>a  </p><p>a  <br>b</p><p><span class=ib>a  </span></p>",
    );
    let runs = placed_runs(&pages[0].fragments[0]);
    let space = runs
        .iter()
        .find(|run| run.3 == "a  ")
        .map(|run| run.2)
        .expect("a run");
    let ends: Vec<f32> = runs
        .iter()
        .filter(|run| run.3.starts_with('a'))
        .map(|run| run.0 + run.2)
        .collect();
    assert_eq!(ends.len(), 3, "{runs:?}");
    for end in ends {
        assert!((end - 200.0).abs() < 0.5, "{runs:?} {space}");
    }
}

#[test]
fn keeps_lines_that_do_not_wrap_beside_floats() {
    // A line of a box whose lines do not wrap overflows beside a float, as in Chrome; one
    // that wraps goes below it.
    let lines = |white_space: &str| {
        let pages = render(&format!(
            "<style>@page {{ margin: 0 }} body {{ font: 20px/20px Inter; margin: 0 }}
               .f {{ float: left; width: 50px; height: 40px }} p {{ margin: 0; width: 100px; white-space: {white_space} }}</style>
             <div class=f></div><p>Unbreakable</p>"
        ));
        let mut lines = Vec::new();
        line_tops(&pages[0].fragments[0], 0.0, &mut lines);
        lines
    };
    assert_eq!(lines("nowrap")[0].0, 0.0);
    assert_eq!(lines("pre")[0].0, 0.0);
    assert_eq!(lines("normal")[0].0, 40.0);
}

#[test]
fn hangs_other_space_separators_at_the_end_of_a_line() {
    // An ideographic space after a word hangs: the words before it stay on their line (whose
    // text is trimmed of it here).
    let pages = render(
        "<style>@page { margin: 0 } body { font: 20px/20px Inter; margin: 0 } p { margin: 0; width: 40px }</style>
         <p>a b\u{3000}cd</p>",
    );
    let mut lines = Vec::new();
    line_texts(&pages[0].fragments[0], &mut lines);
    assert_eq!(lines, ["a b", "cd"]);
}

#[test]
fn forces_no_break_at_other_line_breaking_characters() {
    let lines = |body: &str| {
        let pages = render(&format!(
            "<style>@page {{ margin: 0 }} body {{ font: 20px/20px Inter; margin: 0 }}
               .pre {{ white-space: pre }} .narrow {{ width: 1px }}</style>{body}"
        ));
        let mut lines = Vec::new();
        line_texts(&pages[0].fragments[0], &mut lines);
        lines
    };
    // A line tabulation, a form feed and line and paragraph separators stay on the line; a form
    // feed is no collapsible space, but goes where spaces are preserved.
    assert_eq!(
        lines("<p>a\u{B}b\u{C}c\u{2028}d\u{2029}e</p>"),
        ["a\u{B}b\u{C}c\u{2028}d\u{2029}e"]
    );
    assert_eq!(lines("<p class=pre>a\u{C}b</p>"), ["ab"]);
    // A line may break after a separator (which `line_texts` trims), but not at a line
    // tabulation or a form feed.
    assert_eq!(
        lines("<p class=narrow>a\u{B}b\u{C}c\u{2028}d</p>"),
        ["a\u{B}b\u{C}c", "d"]
    );
}

#[test]
fn breaks_japanese_between_phrases_with_auto_phrase() {
    let lines = |body: &str| {
        let pages = render(&format!(
            "<style>@page {{ margin: 0 }} body {{ font: 20px/20px Inter; margin: 0 }}
               p {{ word-break: auto-phrase; margin: 0 }}</style>{body}"
        ));
        let mut lines = Vec::new();
        line_texts(&pages[0].fragments[0], &mut lines);
        lines
    };
    // As narrow as the longest phrase: the phrases go on lines of their own.
    assert_eq!(
        lines("<p lang=ja style='width: min-content'>東京へ行きましょう。</p>"),
        ["東京へ", "行きましょう。"]
    );
    // A phrase that does not fit breaks as `normal` does, at soft hyphens too, which phrases
    // do not take otherwise.
    assert_eq!(
        lines("<p lang=ja style='width: 1px'>ドライブ</p>"),
        ["ド", "ラ", "イ", "ブ"]
    );
    // (The hyphen shows the soft hyphen's text.)
    assert_eq!(
        lines("<p lang=en style='width: 1px'>con\u{AD}sid</p>"),
        ["con\u{AD}", "sid"]
    );
    assert_eq!(
        lines("<p lang=en style='width: min-content'>con\u{AD}sid</p>"),
        ["consid"]
    );
}

#[test]
fn removes_segment_breaks_next_to_zero_width_spaces() {
    let lines = |body: &str| {
        let pages = render(&format!(
            "<style>@page {{ margin: 0 }} body {{ font: 20px/20px Inter; margin: 0 }}</style>{body}"
        ));
        let mut lines = Vec::new();
        line_texts(&pages[0].fragments[0], &mut lines);
        lines
    };
    // Before or after the spaces, in the same text or across inline boxes and floats.
    assert_eq!(lines("<p>aa\u{200B}\n  bb</p>"), ["aa\u{200B}bb"]);
    assert_eq!(lines("<p>aa \n\u{200B}bb</p>"), ["aa\u{200B}bb"]);
    assert_eq!(lines("<p>aa<span>\u{200B}</span>\nbb</p>"), ["aa\u{200B}bb"]);
    assert_eq!(lines("<p>aa\n<span>\u{200B}bb</span></p>"), ["aa\u{200B}bb"]);
    // Spaces without a segment break stay.
    assert_eq!(lines("<p>aa\u{200B} bb</p>"), ["aa\u{200B} bb"]);
}

#[test]
fn starts_the_line_after_a_preserved_newline_without_collapsible_space() {
    let pages = render(
        "<style>@page { margin: 0 } body { font: 20px/20px Inter; margin: 0 } .pre { white-space: pre }</style>
         <p><span class=pre>ab\n</span><span> cd</span></p>",
    );
    let mut lines = Vec::new();
    line_texts(&pages[0].fragments[0], &mut lines);
    let runs = placed_runs(&pages[0].fragments[0]);
    let cd = runs.iter().find(|run| run.3.contains("cd")).expect("cd");
    assert_eq!(cd.3, "cd", "{runs:?}");
    assert!(cd.0.abs() < 0.01, "{runs:?}");
}

#[test]
fn places_initial_letters_as_chrome_does() {
    let (document, pages) = render_document(
        "<style>@page { margin: 0 } body { font: 20px/24px Inter; margin: 0 } p { width: 300px; margin: 0 }
           p::first-letter { initial-letter: 3 } .indent { text-indent: 10px }
           .float { float: left; width: 30px; height: 100px }
           .after { display: flow-root; height: 10px } .plain { height: 10px }</style>
         <p class=indent>Lorem<br>ipsum<br>dolor<br>sit</p>
         <div><div class=float></div><p>Quis<br>autem<br>vel</p></div>
         <p>Short</p><div class=after></div>
         <p>Brief</p><div class=plain></div>",
    );
    let runs = placed_runs(&pages[0].fragments[0]);
    let run = |text: &str| runs.iter().find(|run| run.3 == text).expect("a run");
    // After the text indent, which its own box (inheriting it) has again; the lines beside it
    // start where its box ends, the first line without the indent, and the fourth line, below
    // the letter's ink, at the start.
    assert!(run("L").0 > 10.0, "{runs:?}");
    assert!((run("orem").0 - run("ipsum").0).abs() < 0.01, "{runs:?}");
    assert!((run("ipsum").0 - run("dolor").0).abs() < 0.01, "{runs:?}");
    assert!(run("sit").0.abs() < 0.01, "{runs:?}");
    // Beside a float at the start of its line, with the lines beside both.
    assert!(run("Q").0 > 20.0, "{runs:?}");
    assert!(run("autem").0 > run("Q").0 + run("Q").2 * 0.5, "{runs:?}");
    // A new formatting context goes below the letter of the paragraph before; a block that is
    // none does not.
    let below = |class: &str, text: &str| find(&pages[0], &document, class).expect("the box").1 - run(text).1;
    assert!(below("after", "hort") > 40.0, "{runs:?}");
    assert!(below("plain", "rief") < 15.0, "{runs:?}");
}

#[test]
fn removes_the_space_after_a_first_letter_taken_out_of_the_line() {
    // Initial letters and floated first letters start the line beside them with the text after
    // the space that follows them, which the line's start removes, as in Chrome: the text starts
    // where it does without the space. An inline first letter keeps it.
    let (_, pages) = render_document(
        "<style>@page { margin: 0 } body { font: 20px/24px Inter; margin: 0 }
           p { width: 300px; margin: 0; display: flow-root }
           .initial::first-letter { initial-letter: 2 } .float::first-letter { float: left; font-size: 40px }
           .inline::first-letter { font-weight: 700 }</style>
         <p class=initial>A one</p><p class=initial>Atwo</p>
         <p class=float>B three</p><p class=float>Bfour</p>
         <p class=inline>C five</p>",
    );
    let runs = placed_runs(&pages[0].fragments[0]);
    let run = |text: &str| runs.iter().find(|run| run.3 == text).expect("a run");
    assert!((run("one").0 - run("two").0).abs() < 0.01, "{runs:?}");
    assert!((run("three").0 - run("four").0).abs() < 0.01, "{runs:?}");
    assert!(runs.iter().any(|run| run.3 == " five"), "{runs:?}");
}

#[test]
fn trims_inline_boxes_and_stops_trimming_at_padding() {
    let (document, pages) = render_document(
        "<style>@page { margin: 0 } body { font: 20px/60px Inter; margin: 0 }
           .t { text-box: trim-both cap alphabetic } .p { text-box: trim-both text } .inner { padding-top: 1px }</style>
         <p><span class=t>Hx</span></p>
         <div class=p><div class=inner>first</div></div>",
    );
    // The inline box reaches from the cap height to the baseline (Inter's cap height is 0.727
    // em).
    let trimmed = find(&pages[0], &document, "t").expect("the span").2;
    assert!((trimmed.rect.height.to_px() - 14.54).abs() < 0.1, "{trimmed:?}");
    // Padding between the block that trims and its first line: no effect, the half-leading
    // stays above the text.
    let runs = placed_runs(&pages[0].fragments[0]);
    let first = runs.iter().find(|run| run.3 == "first").expect("a run");
    let inner = find(&pages[0], &document, "inner").expect("the box").1;
    assert!(first.1 - inner > 30.0, "{runs:?}");
}

#[test]
fn breaks_at_soft_hyphens_at_the_start_of_an_item() {
    // Wherever the inline boxes start and end around it, as in Chrome: the line ends with
    // "high" and a hyphen.
    for content in [
        "high&shy;way",
        "<span>high</span>&shy;way",
        "high<span>&shy;</span>way",
        "high<span>&shy;way</span>",
        "<span>high</span>&shy;<span>way</span>",
    ] {
        let pages = render(&format!(
            "<style>body {{ font: 16px Inter; margin: 0 }} div {{ width: 50px }}</style><div>{content}</div>"
        ));
        let mut lines = Vec::new();
        line_texts(&pages[0].fragments[0], &mut lines);
        assert_eq!(lines.len(), 2, "{content}: {lines:?}");
        assert!(
            lines[0].starts_with("high") && lines[0].len() > 4,
            "{content}: {lines:?}"
        );
        assert_eq!(lines[1], "way", "{content}: {lines:?}");
    }
}

#[test]
fn breaks_after_text_that_does_not_wrap_where_the_box_around_wraps() {
    // The nearest common ancestor of the space that ends a `nowrap` box and what follows
    // decides: a line breaks between two such boxes in a paragraph that wraps, not inside one.
    assert_eq!(
        narrow_lines("<span class=n>a </span> <span class=n>b </span> <span class=n>c d</span>"),
        ["a", "b", "c d"]
    );
}

#[test]
fn takes_the_spaces_that_hang_at_the_end_of_a_line_into_boxes_that_go_on() {
    // A box that goes on to the next line ends after the preserved spaces and other space
    // separators that hang at the end of its line, but before collapsible spaces, which are
    // removed, as in Chrome.
    let width = |content: &str| {
        let (document, pages) = render_document(&format!(
            "<style>@page {{ margin: 0 }} body {{ font: 20px/20px Inter; margin: 0 }}
               p {{ margin: 0; width: 300px }} .pre {{ white-space: pre-wrap }}</style>
             <p>{content}</p>"
        ));
        find(&pages[0], &document, "s")
            .expect("the box")
            .2
            .rect
            .width
            .to_px()
    };
    let a = width("<span class=s>a<br>b</span>");
    assert!((width("<span class=s>a <br>b</span>") - a).abs() < 0.01);
    assert!(width("<span class=s>a\u{3000}<br>b</span>") > a + 15.0);
    assert!(width("<span class='s pre'>a   <br>b</span>") > a + 10.0);
}

#[test]
fn spills_lines_too_wide_for_their_box_to_the_left_right_to_left() {
    // Whatever their alignment, as in Blink.
    for align in ["left", "center", "right"] {
        let pages = render(&format!(
            "<style>@page {{ margin: 0 }} body {{ font: 20px/20px Inter; margin: 0 }}
               p {{ margin: 0 0 0 200px; width: 40px; white-space: nowrap; text-align: {align} }}</style>
             <p dir=rtl>abcdefgh</p>"
        ));
        let runs = placed_runs(&pages[0].fragments[0]);
        let end = runs.iter().map(|run| run.0 + run.2).fold(f32::MIN, f32::max);
        assert!((end - 240.0).abs() < 0.5, "{align}: {runs:?}");
    }
}

#[test]
fn makes_line_boxes_of_inline_boxes_with_inline_margins_borders_or_padding() {
    // An empty inline box with padding, a border or a margin (even a negative one) at its
    // sides makes a line box, which keeps the margins of the block after it from collapsing
    // through its block; one with them only above and below does not.
    let top = |span: &str| {
        let (document, pages) = render_document(&format!(
            "<style>@page {{ margin: 0 }} body {{ margin: 0 }} .w > div {{ line-height: 0 }}
               .after {{ display: flow-root; margin-top: 100px; height: 10px }}</style>
             <div class=w><div class=inner><span style='{span}'></span><div class=after></div></div></div>"
        ));
        find(&pages[0], &document, "inner").expect("the block").1
    };
    assert_eq!(top("padding-top: 1px"), 100.0);
    assert_eq!(top("padding-left: 1px"), 0.0);
    assert_eq!(top("margin-right: -1px"), 0.0);
    assert_eq!(top("border-left: 1px solid"), 0.0);
}

#[test]
fn indents_each_line_and_hangs_the_indent() {
    let starts = |indent: &str| {
        let pages = render(&format!(
            "<style>@page {{ margin: 0 }} body {{ font: 20px/20px Inter; margin: 0 }}
               p {{ margin: 0; width: 80px; text-indent: 30px {indent} }}</style>
             <p>aaaa bbbb<br>cccc</p>"
        ));
        let mut runs = placed_runs(&pages[0].fragments[0]);
        runs.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.total_cmp(&b.0)));
        let mut starts: Vec<f32> = Vec::new();
        let mut last_baseline = f32::MIN;
        for run in runs {
            if (run.1 - last_baseline).abs() > 0.01 {
                starts.push(run.0);
                last_baseline = run.1;
            }
        }
        starts
    };
    assert_eq!(starts(""), [30.0, 0.0, 0.0]);
    assert_eq!(starts("each-line"), [30.0, 0.0, 30.0]);
    assert_eq!(starts("hanging"), [0.0, 30.0, 30.0]);
    assert_eq!(starts("each-line hanging"), [0.0, 30.0, 0.0]);
}

#[test]
fn breaks_between_inline_boxes_for_the_min_content_width_where_words_break_anywhere() {
    let (document, pages) = render_document(
        "<style>@page { margin: 0 } body { font: 20px/20px Inter; margin: 0 }
           .f { float: left; overflow-wrap: anywhere }</style>
         <div style='width: 0'><div class=f><span>W</span><span>W</span><span>W</span></div></div>",
    );
    let float = find(&pages[0], &document, "f").expect("the float").2;
    assert!(float.rect.width.to_px() < 25.0, "{:?}", float.rect);
    assert!(float.rect.height.to_px() > 55.0, "{:?}", float.rect);
}

#[test]
fn sizes_a_float_around_floats_that_do_not_wrap_by_the_widest() {
    // Each float is as wide as it is on its own, also where its lines do not wrap: floats join
    // no words of the line for the min-content width (WPT letter-spacing-206).
    let (document, pages) = render_document(
        "<style>@page { size: 800px 600px; margin: 0 } body { margin: 0 }
         .c { float: left } .f { float: left; clear: left; white-space: pre; font: 20px Inter }</style>
         <div class='c one'>
<p class='f a'>AAA BBB</p>
<p class='f b'>CC</p>
</div>",
    );
    let (_, _, container) = find(&pages[0], &document, "one").expect("container");
    let (_, _, widest) = find(&pages[0], &document, "a").expect("float");
    assert_eq!(container.rect.width, widest.rect.width);
}

#[test]
fn indents_only_the_anonymous_block_that_starts_its_element() {
    // Text after a block in the flow is in an anonymous block that is not the first line of
    // its element: it is not indented (text-indent-014); one after an out-of-flow box is.
    let (document, pages) = render_document(
        "<style>@page { margin: 0 } body { font: 16px Inter; margin: 0 } div { text-indent: 50px }</style>
         <div><span class=a>One</span><div>Two</div><span class=b>Three</span></div>
         <div><div style='position: absolute'>Four</div><span class=c>Five</span></div>",
    );
    let x = |class: &str| find(&pages[0], &document, class).expect(class).0;
    assert_eq!((x("a"), x("b"), x("c")), (50.0, 0.0, 50.0));
}

#[test]
fn takes_the_spaces_that_never_collapse_into_the_first_letter() {
    // A no-break or em space before the letter is part of it, as in Chrome
    // (first-letter-space-not-selected); a collapsible space is not.
    let (_, pages) = render_document(
        "<style>@page { margin: 0 } body { font: 20px/24px Inter; margin: 0 }
           p::first-letter { font-weight: 700 }</style>
         <p>&emsp;A one</p><p>&nbsp;B two</p><p> C three</p>",
    );
    let runs = placed_runs(&pages[0].fragments[0]);
    for letter in ["\u{2003}A", "\u{a0}B", "C"] {
        assert!(runs.iter().any(|run| run.3 == letter), "{letter:?} in {runs:?}");
    }
}

#[test]
fn takes_no_first_letter_after_a_block_without_lines() {
    fn sizes(fragment: &Fragment, out: &mut Vec<(String, f32)>) {
        if let FragmentKind::Text(run) = &fragment.kind {
            out.push((run.text.clone(), run.style.font_size));
        }
        for child in &fragment.children {
            sizes(child, out);
        }
    }
    // The first formatted line lies in the empty block, so the text after it has no first
    // letter, as in Chrome (WPT `css-pseudo/first-letter-require-first-formatted-line`).
    let pages = render(
        "<style>body { font: 16px Inter } div::first-letter { font-size: 40px }</style>
         <div><div></div>Xyz</div><div>Abc</div>",
    );
    let mut found = Vec::new();
    sizes(&pages[0].fragments[0], &mut found);
    assert!(
        found
            .iter()
            .any(|(text, size)| text == "A" && (*size - 40.0).abs() < 0.01),
        "{found:?}"
    );
    assert!(
        !found
            .iter()
            .any(|(text, size)| text.starts_with('X') && (*size - 40.0).abs() < 0.01),
        "{found:?}"
    );
}

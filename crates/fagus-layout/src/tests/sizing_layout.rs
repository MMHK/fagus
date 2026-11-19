//! Intrinsic and extrinsic sizing as Chrome does it: sizing keywords in widths, minimums and
//! maximums, size contributions, flex and grid items' automatic minimum sizes, table widths and
//! heights, aspect ratios and size containment. The numbers are Chrome's, measured with the
//! same font.

use super::{find, render_document};

/// The positions and sizes of the boxes with the given classes, in CSS px.
fn boxes(html: &str, classes: &[&str]) -> Vec<(f32, f32, f32, f32)> {
    let (document, pages) = render_document(&format!(
        "<!DOCTYPE html><style>body {{ margin: 0; font: 12px/16px Inter }}</style>{html}"
    ));
    classes
        .iter()
        .map(|class| {
            let (x, y, fragment) = find(&pages[0], &document, class).expect(class);
            (x, y, fragment.rect.width.to_px(), fragment.rect.height.to_px())
        })
        .collect()
}

fn widths(html: &str, classes: &[&str]) -> Vec<f32> {
    boxes(html, classes)
        .into_iter()
        .map(|(_, _, width, _)| width)
        .collect()
}

const TEXT: &str = "Lorem ipsum dolor sit amet consectetur";

#[test]
fn sizes_blocks_by_keywords() {
    let html = format!(
        "<div style='width: 400px'>
           <div class=min style='width: min-content'>{TEXT}</div>
           <div class=max style='width: max-content'>{TEXT}</div>
           <div class=fit style='width: fit-content; margin: 0 auto'>Two words</div>
           <div class=stretch style='width: stretch; margin: 0 10px'>x</div>
           <div class=bounded style='width: 390px; max-width: min-content'>{TEXT}</div>
           <div class=floor style='width: 10px; min-width: max-content'>Two words</div>
         </div>"
    );
    let found = boxes(&html, &["min", "max", "fit", "stretch", "bounded", "floor"]);
    let (min, max) = (found[0].2, found[1].2);
    assert!(min < 80.0 && max > 200.0, "{min} {max}");
    // `fit-content` with auto margins centres the box.
    let (x, _, fit, _) = found[2];
    let left = found[1].0;
    assert!((x - left - (400.0 - fit) / 2.0).abs() < 0.1, "{found:?}");
    assert!((found[3].2 - 380.0).abs() < 0.1);
    assert!((found[4].2 - min).abs() < 0.1);
    assert!((found[5].2 - fit).abs() < 0.1);
}

#[test]
fn sizes_shrink_to_fit_boxes_by_keywords() {
    let html = format!(
        "<div style='position: relative; width: 300px; height: 100px'>
           <div class=float style='float: left; width: min-content'>{TEXT}</div>
           <div class=abs style='position: absolute; left: 0; right: 0; width: min-content'>{TEXT}</div>
           <span class=ib style='display: inline-block; width: stretch'>x</span>
         </div>
         <div class=block style='width: min-content'>{TEXT}</div>"
    );
    let found = widths(&html, &["float", "abs", "ib", "block"]);
    assert!((found[0] - found[3]).abs() < 0.1);
    assert!((found[1] - found[3]).abs() < 0.1);
    assert!((found[2] - 300.0).abs() < 0.1);
}

#[test]
fn clamps_contributions_by_minimums_and_maximums() {
    // The float shrinks to its child's maximum, and grows to another child's minimum.
    let html = format!(
        "<div style='width: 400px'>
           <div class=narrow style='float: left'><div style='max-width: 100px'>{TEXT}</div></div>
         </div>
         <div style='width: 400px; clear: both'>
           <div class=wide style='float: left'><div style='min-width: 300px'>x</div></div>
         </div>"
    );
    assert_eq!(widths(&html, &["narrow", "wide"]), [100.0, 300.0]);
}

#[test]
fn takes_fit_content_functions_in_sizes_as_invalid() {
    // Chrome does not take `fit-content(<length>)` in `width`: the box fills its container.
    let found = widths(
        "<div style='width: 400px'><div class=f style='width: fit-content(100px)'>x</div></div>",
        &["f"],
    );
    assert_eq!(found, [400.0]);
}

#[test]
fn sizes_tables_by_keywords_and_minimums() {
    let cells = format!("<tr><td>{TEXT}</td><td>Two words</td></tr>");
    let html = format!(
        "<div style='width: 400px'>
           <table class=min style='width: min-content'>{cells}</table>
           <table class=auto>{cells}</table>
           <table class=full style='width: 50px; min-width: 100%'>{cells}</table>
           <table class=capped style='width: 390px; max-width: max-content'>{cells}</table>
         </div>"
    );
    let found = widths(&html, &["min", "auto", "full", "capped"]);
    assert!(found[0] < 150.0);
    assert_eq!(found[2], 400.0);
    assert!((found[3] - found[1]).abs() < 0.1);
}

#[test]
fn distributes_a_table_height_over_its_rows() {
    // Rows of one and three lines (16px and 48px) in 200px: in proportion, as in Chrome; a fixed
    // row keeps its height, and a percentage row takes its share first.
    let html = format!(
        "<table style='border-spacing: 0; height: 200px; width: 200px'>
           <tr><td class=a style='padding: 0'>a</td></tr>
           <tr><td class=b style='padding: 0'>{TEXT} {TEXT}</td></tr></table>
         <table style='border-spacing: 0; height: 200px; width: 200px'>
           <tr style='height: 30px'><td class=c style='padding: 0'>a</td></tr>
           <tr><td class=d style='padding: 0'>b</td></tr></table>
         <table style='border-spacing: 0; height: 200px; width: 200px'>
           <tr style='height: 50%'><td class=e style='padding: 0'>a</td></tr>
           <tr><td class=f style='padding: 0'>b</td></tr></table>"
    );
    let heights: Vec<f32> = boxes(&html, &["a", "b", "c", "d", "e", "f"])
        .into_iter()
        .map(|(_, _, _, height)| height)
        .collect();
    assert_eq!(heights, [50.0, 150.0, 30.0, 170.0, 100.0, 100.0]);
}

#[test]
fn resolves_percentage_heights_in_cells_of_fixed_height() {
    let found = boxes(
        "<table style='border-spacing: 0; height: 90px'><tr><td style='padding: 0'>
           <div class=full style='height: 100%'>a</div></td></tr></table>
         <table style='border-spacing: 0'><tr><td style='padding: 0; height: 90px'>
           <div class=half style='height: 50%'>a</div></td></tr></table>",
        &["full", "half"],
    );
    assert_eq!((found[0].3, found[1].3), (90.0, 45.0));
}

#[test]
fn gives_flex_items_their_automatic_minimum_size() {
    let word = "Supercalifragilisticexpialidocious";
    let html = format!(
        "<div style='display: flex; width: 150px'><div class=auto>{word}</div><div style='flex: none; width: 100px'>x</div></div>
         <div style='display: flex; width: 150px'><div class=hidden style='overflow: hidden'>{word}</div><div style='flex: none; width: 100px'>x</div></div>
         <div style='display: flex; width: 150px'><div class=zero style='min-width: 0'>{word}</div><div style='flex: none; width: 100px'>x</div></div>
         <div style='display: flex; width: 400px'><div class=basis style='flex: 0 0 0'>{TEXT}</div></div>"
    );
    let found = widths(&html, &["auto", "hidden", "zero", "basis"]);
    assert!(found[0] > 150.0, "the long word sets the minimum: {}", found[0]);
    assert_eq!(found[1..3], [50.0, 50.0]);
    // A zero basis cannot take the item below its min-content width.
    let min = widths(
        &format!("<div class=m style='width: min-content'>{TEXT}</div>"),
        &["m"],
    )[0];
    assert!((found[3] - min).abs() < 0.1);
}

#[test]
fn takes_sizing_keywords_as_flex_bases() {
    let html = format!(
        "<div style='display: flex; width: 400px'><div class=a style='flex: 1 1 min-content'>{TEXT}</div><div class=b style='flex: 1 1 min-content'>xy</div></div>
         <div style='display: flex; width: 400px'><div class=c style='flex-basis: max-content; flex-shrink: 0'>{TEXT}</div></div>"
    );
    let found = widths(&html, &["a", "b", "c"]);
    // Both grow by the same amount from their min-content widths.
    let min = widths(
        &format!("<div class=m style='width: min-content'>{TEXT}</div>"),
        &["m"],
    )[0];
    let xy = widths("<span class=m style='display: inline-block'>xy</span>", &["m"])[0];
    assert!(((found[0] - min) - (found[1] - xy)).abs() < 0.1, "{found:?}");
    let max = widths(
        &format!("<div class=m style='width: max-content'>{TEXT}</div>"),
        &["m"],
    )[0];
    assert!((found[2] - max).abs() < 0.1);
}

#[test]
fn freezes_flex_items_at_their_maximums() {
    // The first item stops at 50px; the second takes the rest of the free space.
    let found = widths(
        "<div style='display: flex; width: 400px'><div class=a style='flex: 1; max-width: 50px'></div><div class=b style='flex: 1'></div></div>",
        &["a", "b"],
    );
    assert_eq!(found, [50.0, 350.0]);
}

#[test]
fn sizes_flex_items_through_their_aspect_ratio() {
    // A stretched item with a ratio takes its width from the line's height; so does an image.
    let found = boxes(
        "<div style='display: flex; width: 300px; height: 50px'><div class=ratio style='aspect-ratio: 2'></div></div>
         <div style='display: flex; flex-direction: column; width: 100px'><div class=column style='height: 20px'></div></div>",
        &["ratio", "column"],
    );
    assert_eq!((found[0].2, found[0].3), (100.0, 50.0));
    assert_eq!(found[1].2, 100.0);
}

#[test]
fn sizes_column_flex_items_in_a_fixed_height() {
    let html = format!(
        "<div style='display: flex; flex-direction: column; width: 200px; height: 80px'>
           <div class=half style='height: 50%'>a</div><div>b</div></div>
         <div style='display: flex; flex-direction: column; width: 200px; height: 40px'>
           <div class=shrunk style='overflow: hidden'>{TEXT} {TEXT}</div><div style='flex: none; height: 30px'>x</div></div>"
    );
    let found = boxes(&html, &["half", "shrunk"]);
    assert_eq!((found[0].3, found[1].3), (40.0, 10.0));
}

#[test]
fn gives_grid_items_their_minimum_contribution() {
    let word = "Supercalifragilisticexpialidocious";
    let found = widths(
        &format!(
            "<div style='display: grid; grid-template-columns: 1fr 50px; width: 150px'><div class=auto>{word}</div><div>a</div></div>
             <div style='display: grid; grid-template-columns: 1fr 50px; width: 150px'><div class=zero style='min-width: 0'>{word}</div><div>a</div></div>
             <div style='display: grid; grid-template-columns: minmax(auto, 50px) 1fr; width: 400px'><div class=capped>{TEXT}</div><div>a</div></div>"
        ),
        &["auto", "zero", "capped"],
    );
    assert!(found[0] > 150.0);
    assert_eq!(found[1..], [100.0, 50.0]);
}

#[test]
fn sizes_boxes_with_size_containment_as_if_empty() {
    let html = format!(
        "<div style='width: 300px'>
           <div class=empty style='display: inline-block; contain: size'>{TEXT}</div>
           <div class=given style='display: inline-block; contain: size; contain-intrinsic-size: 80px 20px'>{TEXT}</div>
           <div class=query style='float: left; container-type: inline-size'>{TEXT}</div>
         </div>"
    );
    let found = boxes(&html, &["empty", "given", "query"]);
    assert_eq!((found[0].2, found[0].3), (0.0, 0.0));
    assert_eq!((found[1].2, found[1].3), (80.0, 20.0));
    assert_eq!(found[2].2, 0.0);
}

#[test]
fn shrinks_boxes_with_a_ratio_to_their_height() {
    let found = boxes(
        "<div style='width: 300px'><div class=float style='float: left; height: 30px; aspect-ratio: 2'>Text in a box</div></div>
         <div style='width: 300px; clear: both'><div class=clipped style='width: 50px; aspect-ratio: 1; overflow: hidden'>Text in a box with some words</div></div>",
        &["float", "clipped"],
    );
    assert_eq!((found[0].2, found[0].3), (60.0, 30.0));
    // A scroll container has no automatic minimum height: the ratio wins over the content.
    assert_eq!(found[1].3, 50.0);
}

#[test]
fn widens_shrink_to_fit_boxes_for_percentage_columns() {
    // A 40px column of 25% makes the table 160px wide in a float, but not in a flex item or a
    // cell, where Chrome leaves percentages out of the table's intrinsic width.
    let table = "<table style='border-spacing: 0'><tr><td style='width: 25%; padding: 0'>\
                 <div style='width: 40px; height: 10px'></div></td><td style='padding: 0'>\
                 <div style='width: 30px; height: 10px'></div></td></tr></table>";
    let html = format!(
        "<div class=float style='float: left'>{table}</div>
         <div style='clear: both; display: flex'><div class=item>{table}</div></div>
         <table><tr><td style='padding: 0'><div class=nested style='float: left'>{table}</div></td></tr></table>"
    );
    assert_eq!(widths(&html, &["float", "item", "nested"]), [160.0, 70.0, 70.0]);
}

#[test]
fn starts_select_text_at_the_top_where_it_does_not_fit() {
    // The text of a menu list is centred, but starts at the top where the select is too low for
    // it; the select's baseline is that of its text.
    let html = "<div class=line>x<select class=low style='height: 12px; border: 0; padding: 0'>\
                <option>Agj</option></select></div>\
                <div class=line2>x<select class=high style='height: 40px; border: 0; padding: 0'>\
                <option>Agj</option></select></div>";
    let found = boxes(html, &["line", "low", "line2", "high"]);
    // The select's baseline lies further below its top than the strut's below the line's top:
    // the select starts the line, which grows.
    assert!((found[1].1 - found[0].1).abs() < 0.01, "{found:?}");
    assert!(found[0].3 > 16.5, "{found:?}");
    // Centred: 40px high, the select reaches above and below the strut.
    assert!(found[3].1 - found[2].1 < 0.01, "{found:?}");
}

#[test]
fn stretches_fields_in_flex_columns_and_grids_but_not_in_block_flow() {
    // A text field, date field or select is as wide as its `size` or options, but its width
    // stays auto: Chrome stretches it in a flex column and a grid, and fits it to that width in
    // block flow, even with `display: block`. A checkbox stretched keeps its height.
    let html = "<div><input class=inline></div>
                <div style='display: flex; flex-direction: column; width: 300px'>
                  <input class=column><input class=date type=date><select class=select><option>a</option></select>
                  <input class=check type=checkbox></div>
                <div style='display: grid; width: 300px'><input class=grid></div>
                <div style='width: 300px'><input class=block style='display: block'></div>";
    let found = boxes(
        html,
        &["inline", "column", "date", "select", "check", "grid", "block"],
    );
    let field = found[0].2;
    assert!(field > 100.0 && field < 300.0, "{found:?}");
    for stretched in &found[1..4] {
        assert_eq!(stretched.2, 300.0, "{found:?}");
    }
    // The checkbox: its margins of 4px and 3px at the sides, 13px high.
    assert_eq!((found[4].2, found[4].3), (293.0, 13.0), "{found:?}");
    assert_eq!(found[5].2, 300.0, "{found:?}");
    assert_eq!(found[6].2, field, "{found:?}");
}

#[test]
fn compresses_fields_with_percentage_widths() {
    // A field with a percentage width contributes nothing to the min-content width of what holds
    // it (as a replaced element does), so flex items share the row by their factors alone.
    let html = "<div style='display: flex; width: 300px'>
                  <div class=a style='flex: 2'><input style='width: 100%; box-sizing: border-box'></div>
                  <div class=b style='flex: .5'><input style='width: 100%; box-sizing: border-box'></div>
                </div>";
    let found = boxes(html, &["a", "b"]);
    assert_eq!((found[0].2, found[1].2), (240.0, 60.0), "{found:?}");
}

#[test]
fn measures_ch_in_variable_fonts_at_their_weight_and_optical_size() {
    // Chrome measures the "0" of a variable font in the instance text is set in: SF Pro's at
    // 16px is that of its optical size 17, wider than at its default 28. Inter's "0" is 1292
    // units wide at `opsz` 14, 1256 at 32, and 1381 bold at 14 (of 2048).
    use std::sync::Arc;

    use fagus_style::values::{FontMetrics, FontRequest, FontStyle};
    use fagus_text::{Descriptors, Face, FontDb, unpack};
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/webfonts/Inter-Variable-Latin.woff");
    let data = Arc::new(unpack(std::fs::read(path).expect("font")).expect("unpacked"));
    let rule = Descriptors {
        family: "Variable".to_owned(),
        ..Descriptors::default()
    };
    let mut fonts = FontDb::new();
    fonts.add_document_face(Arc::new(Face::from_rule(data, 0, &rule).expect("face")));
    let ch = |weight: f32, optical_size: Option<f32>| {
        crate::inline::StyleFonts(&fonts)
            .units(&FontRequest {
                families: Arc::from(["Variable".to_owned()]),
                weight,
                style: FontStyle::Normal,
                stretch: 100.0,
                optical_size,
                size: 16.0,
                lang: None,
            })
            .ch
    };
    let close = |value: f32, units: f32| (value - units / 2048.0).abs() < 1e-4;
    assert!(close(ch(400.0, Some(8.0)), 1292.0), "{}", ch(400.0, Some(8.0)));
    assert!(close(ch(400.0, Some(40.0)), 1256.0), "{}", ch(400.0, Some(40.0)));
    assert!(close(ch(700.0, Some(14.0)), 1381.0), "{}", ch(700.0, Some(14.0)));
}

#[test]
fn limits_auto_widths_by_a_maximum_height_through_the_ratio() {
    let html =
        "<div style='width: 300px'><div class=ratio style='aspect-ratio: 2; max-height: 20px'></div></div>";
    let (_, _, width, height) = boxes(html, &["ratio"])[0];
    assert_eq!((width, height), (40.0, 20.0));
}

#[test]
fn gives_fixed_table_columns_their_width_before_auto_columns_grow() {
    // A long text in an auto column wants more than the table has: the columns with a width
    // keep it, as in Chrome, and the auto column takes what is left.
    let long = TEXT.repeat(6);
    let html = format!(
        "<table style='width: 600px; border-collapse: collapse'>
           <tr><td class=a style='width: 30px; padding: 0'>1</td><td class=b style='padding: 0'>{long}</td><td class=c style='width: 100px; padding: 0'>x</td></tr>
         </table>"
    );
    let found = widths(&html, &["a", "b", "c"]);
    assert_eq!(found, [30.0, 470.0, 100.0]);
}

#[test]
fn measures_words_for_the_min_content_width_between_rounded_places() {
    // Chrome measures "1044" after "A-" from the place where it starts in the text, rounded up
    // to layout units, to where it ends, rounded up: 27.953125 pixels, though it is 27.955 wide
    // and as a text of its own 27.96875 (measured).
    let html = "<style>div { display: inline-block; width: min-content; font-size: 9pt }</style>
                <div class=hyphen>A-1044</div><br><div class=alone>1044</div>";
    assert_eq!(widths(html, &["hyphen", "alone"]), [27.953_125, 27.968_75]);
}

#[test]
fn stretches_text_areas_sliders_and_checkboxes_in_flex_and_grid() {
    // Chrome stretches a text area, a slider and a checkbox like a block: across a flex column
    // or a grid area, and a text area also down a flex row.
    let html = "<div style='display: flex; flex-direction: column; width: 300px'>
                  <textarea class=column></textarea><input class=slider type=range></div>
                <div style='display: flex; width: 400px'>
                  <textarea class=row></textarea><div style='height: 80px; width: 20px'></div></div>
                <div style='display: grid; width: 300px'><input class=grid type=range></div>
                <div style='display: grid; grid-template-columns: 1fr 1fr; width: 300px'>
                  <input class=check type=checkbox><div style='height: 40px'></div></div>
                <div><textarea class=block></textarea></div>";
    let found = boxes(html, &["column", "slider", "row", "grid", "check", "block"]);
    assert_eq!(found[0].2, 300.0, "{found:?}");
    // The slider's margins of 2px.
    assert_eq!((found[1].2, found[1].3), (296.0, 16.0), "{found:?}");
    assert_eq!(found[2].3, 80.0, "{found:?}");
    assert_eq!(found[3].2, 296.0, "{found:?}");
    // Its margins of 4px and 3px at the sides, 3px above and below.
    assert_eq!((found[4].2, found[4].3), (143.0, 34.0), "{found:?}");
    // In block flow the text area keeps its columns.
    assert!(found[5].2 < 300.0, "{found:?}");
    assert_eq!(found[5].2, found[2].2, "{found:?}");
}

#[test]
fn keeps_room_for_the_scroll_bars_of_text_areas_that_scroll() {
    // Chrome 157 on Linux prints the scroll bars of a text area, 15px thick, where its text
    // overflows it (`overflow: auto`, as `visible` is in it) and with `overflow: scroll`, beside
    // and below the room of its columns and rows; an empty one has none.
    let html = "<p><textarea class=auto></textarea></p>
                <p><textarea class=hidden style='overflow: hidden'></textarea></p>
                <p><textarea class=clip style='overflow-y: clip'></textarea></p>
                <p><textarea class=x style='overflow-x: hidden'></textarea></p>
                <p><textarea class=scroll style='overflow: scroll'></textarea></p>
                <p><textarea class=full rows=1>a\nb\nc</textarea></p>
                <p><textarea class=one rows=1>a</textarea></p>";
    let found = boxes(html, &["auto", "hidden", "clip", "x", "scroll", "full", "one"]);
    assert_eq!(found[0].2, found[1].2, "{found:?}");
    assert_eq!(found[0].2, found[2].2, "{found:?}");
    assert_eq!(found[3].2, found[0].2, "{found:?}");
    assert_eq!(
        (found[4].2 - found[0].2, found[4].3 - found[0].3),
        (15.0, 15.0),
        "{found:?}"
    );
    assert_eq!(
        (found[5].2 - found[6].2, found[5].3 - found[6].3),
        (15.0, 0.0),
        "{found:?}"
    );
}

#[test]
fn sizes_controls_to_their_content_with_field_sizing() {
    // `field-sizing: content`: a text field is as wide as its text (at least the 1px of the
    // caret) and its frame, a text area as its longest line and as high as its lines, a select
    // as its selected option and its arrow, whatever `size`, `cols` and `rows` say.
    let html = "<style>.fs { field-sizing: content }</style>
                <p><input class='fs short' value=ab size=40></p>
                <p><input class='fs long' value=abcd></p>
                <p><input class='fs empty'></p>
                <p><textarea class='fs one' cols=40 rows=5>abcd</textarea></p>
                <p><textarea class='fs two'>abcd
ab</textarea></p>
                <p><select class='fs pick'><option>abcdefgh</option><option selected>ab</option></select></p>
                <p><select class=wide><option>abcdefgh</option><option selected>ab</option></select></p>";
    let found = boxes(html, &["short", "long", "empty", "one", "two", "pick", "wide"]);
    // The frame of a field: a 1px border (the native frame) and 2px of padding on either side.
    assert_eq!(found[2].2, 1.0 + 8.0, "{found:?}");
    assert!(found[0].2 > found[2].2 && found[1].2 > found[0].2, "{found:?}");
    // The same text in a text area (Inter, 2px padding and a 1px border).
    assert_eq!(found[3].2, found[4].2, "{found:?}");
    assert_eq!(found[4].3 - found[3].3, found[3].3 - 6.0, "{found:?}");
    assert!(found[5].2 < found[6].2, "{found:?}");
}

#[test]
fn shows_how_many_options_a_multiple_select_of_size_one_has_selected() {
    // Chrome 157 shows `<select multiple size=1>` as a menu list with its selected option, or
    // else how many are selected, as wide as that text where options are narrower.
    let (document, pages) = render_document(
        "<!DOCTYPE html><style>body { margin: 0; font: 12px/16px Inter }</style>
         <p><select class=none multiple size=1><option>a</option></select></p>
         <p><select class=one multiple size=1><option selected>a</option><option>b</option></select></p>
         <p><select class=two multiple size=1><option selected>a</option><option selected>b</option></select></p>
         <p><select class=list multiple><option>a</option></select></p>",
    );
    let text = |class: &str| super::texts_of(&pages[0], &document, class).join("|");
    assert_eq!(text("none"), "0 selected");
    assert_eq!(text("one"), "a");
    assert_eq!(text("two"), "2 selected");
    let height = |class: &str| {
        find(&pages[0], &document, class)
            .expect(class)
            .2
            .rect
            .height
            .to_px()
    };
    assert!(height("list") > 2.0 * height("one"));
}

#[test]
fn draws_the_arrow_of_a_select_at_the_end_of_its_padding_box() {
    // Chrome 157 puts the arrow 1px inside the padding box, the author's end padding between
    // the text and the arrow; right to left, at the left.
    use crate::fragment::FragmentKind;
    let (document, pages) = render_document(
        "<!DOCTYPE html><style>body { margin: 0; font: 12px/16px Inter }</style>
         <p><select class=plain><option>abc</option></select></p>
         <p><select class=padded style='padding-right: 10px'><option>abc</option></select></p>
         <p><select class=rtl dir=rtl style='padding-left: 10px'><option>abc</option></select></p>",
    );
    // The left and right edges of the arrow in the select's border box.
    let arrow = |class: &str| {
        let fragment = find(&pages[0], &document, class).expect(class).2;
        let icon = fragment
            .children
            .iter()
            .find(|child| {
                child
                    .children
                    .iter()
                    .any(|image| matches!(image.kind, FragmentKind::Image(_)))
            })
            .expect("arrow");
        let x = icon.rect.x.to_px();
        (x, fragment.rect.width.to_px() - x - icon.rect.width.to_px())
    };
    assert_eq!(arrow("plain").1, 2.0);
    assert_eq!(arrow("padded").1, 2.0);
    assert_eq!(arrow("rtl").0, 2.0);
}

#[test]
fn counts_the_minimum_widths_of_cells_in_the_min_content_width_of_tables() {
    // KaTeX's `.vlist-s` cell (`min-width: 2px`) in a table in a box of `min-content` width keeps
    // its two pixels, as in Chrome (Blink's `ComputeColumnInlineConstraints`).
    let found = boxes(
        "<span style='display: inline-block; width: min-content'><span style='display: inline-table'>
           <span class=x style='display: table-cell'>x</span>
           <span class=two style='display: table-cell; min-width: 2px; width: 2px'></span>
           <span class=padded style='display: table-cell; min-width: 10px; padding: 0 3px'></span>
         </span></span>",
        &["x", "two", "padded"],
    );
    assert_eq!((found[1].2, found[2].2), (2.0, 16.0), "{found:?}");
    assert!((found[1].0 - found[0].0 - found[0].2).abs() < 0.01, "{found:?}");
}

#[test]
fn compresses_replaced_elements_of_a_fixed_width_under_a_percentage_maximum() {
    use std::sync::Arc;

    use crate::{Image, ImageFormat, Images};
    // `<img width=330>` under `max-width: 100%` (an SVG chart in a card) contributes nothing to
    // its item's min-content width: two `flex: 1` cards of a 400px row share it equally, and so
    // do the columns of a grid of `1fr 1fr`, as in Chrome.
    let html = "<!DOCTYPE html><style>body { margin: 0; font: 12px/16px Inter }
          .row { display: flex; gap: 10px; width: 400px } .row > div { flex: 1 }
          .grid { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; width: 400px }
          img { display: block; max-width: 100%; height: auto }</style>
        <div class=row><div class=a><img src=chart.png width=330></div><div class=b>B</div></div>
        <div class=grid><div class=c><img src=chart.png width=330></div><div class=d>D</div></div>";
    let (document, pages) = super::render_document_with(html, |_| {
        let mut images = Images::default();
        images.insert(
            "chart.png",
            Arc::new(Image {
                data: Arc::new(Vec::new()),
                format: ImageFormat::Png,
                width: 330.0,
                height: 100.0,
                natural: (true, true),
                ratio: Some(3.3),
                inline: false,
                orientation: 1,
                filters: Arc::from([]),
                pad: 0,
            }),
        );
        images
    });
    for class in ["a", "b", "c", "d"] {
        let (_, _, fragment) = find(&pages[0], &document, class).expect(class);
        assert!(
            (fragment.rect.width.to_px() - 195.0).abs() < 0.1,
            "{class}: {fragment:?}"
        );
    }
}

#[test]
fn sizes_shrink_to_fit_boxes_by_replaced_elements_of_percentage_heights() {
    use std::sync::Arc;

    use crate::{Image, ImageFormat, Images};
    // An image 10 × 10 as tall as its inline block of a fixed height makes it as wide, also in
    // an anonymous block beside a paragraph, and in a float of a border-box height; in a block
    // of an automatic height inside, its percentage refers to nothing (Chrome 157).
    let html = "<!DOCTYPE html><style>body { margin: 0; font: 12px/16px Inter }
          .t { display: inline-block; height: 100px; vertical-align: top }
          .t img, .f img { height: 100% }
          .f { float: left; height: 50px; box-sizing: border-box; padding: 5px }</style>
        <div class='t a'><img src=dot.png></div><div class='t b'><img src=dot.png><p></p></div>
        <div class='t c'><div><img src=dot.png></div></div><div class='f d'><img src=dot.png></div>";
    let (document, pages) = super::render_document_with(html, |_| {
        let mut images = Images::default();
        images.insert(
            "dot.png",
            Arc::new(Image {
                data: Arc::new(Vec::new()),
                format: ImageFormat::Png,
                width: 10.0,
                height: 10.0,
                natural: (true, true),
                ratio: Some(1.0),
                inline: false,
                orientation: 1,
                filters: Arc::from([]),
                pad: 0,
            }),
        );
        images
    });
    let width = |class: &str| {
        let (_, _, fragment) = find(&pages[0], &document, class).expect(class);
        fragment.rect.width.to_px()
    };
    assert_eq!(["a", "b", "c", "d"].map(width), [100.0, 100.0, 10.0, 50.0],);
}

#[test]
fn resolves_percentage_paddings_of_cells_against_the_columns() {
    // Columns of 100px each, 10px apart: the padding of 50% is half of their 210px, though it
    // counts for nothing in the widths of the columns, as in Chrome (and Gecko).
    let html = "<table style='border-spacing: 10px 0'><tr>
          <td style='padding: 0 0 0 50%'><div class=a style='width: 100px; height: 10px'></div></td>
          <td style='padding: 0'><div class=b style='width: 100px; height: 10px'></div></td>
        </tr></table>";
    let found = boxes(html, &["a", "b"]);
    // The second column starts 120px into the table, the first cell's content 115px.
    assert!((found[1].0 - found[0].0 - 5.0).abs() < 0.1, "{found:?}");
}

#[test]
fn keeps_sizing_keywords_of_flex_and_grid_containers_in_flex_and_grid_items() {
    // A flex or grid container of `width: max-content` (a seat label, a badge row) inside a
    // column flex container or a grid does not stretch, as in block flow.
    let html = "<div style='display: flex; flex-direction: column; width: 400px'>
          <div class=flex style='display: flex; width: max-content'><span>Row 7</span><span>Seat 12</span></div>
          <div class=grid style='display: grid; width: min-content'><span>Two words</span></div>
        </div>
        <div style='display: grid; width: 400px'>
          <div class=item style='display: flex; width: fit-content'><span>Row 7</span><span>Seat 12</span></div>
        </div>";
    let found = widths(html, &["flex", "grid", "item"]);
    assert!(found[0] > 20.0 && found[0] < 120.0, "{found:?}");
    assert!(found[1] > 20.0 && found[1] < 80.0, "{found:?}");
    assert!((found[2] - found[0]).abs() < 0.1, "{found:?}");
}

#[test]
fn resolves_percentage_heights_in_stretched_flex_containers() {
    // A card stretched to the 120px of its row (a minimum height) is definite for the
    // percentage heights of its items (CSS Flexbox 1, §9.8), and so is a column item that
    // grew into it, as in Chrome.
    let html = "<div class=row style='display: flex; min-height: 120px; width: 400px'>
          <div style='flex: 1; display: flex; align-items: center'><div class=bar style='width: 4px; height: 50%'></div></div>
          <div style='flex: 1; display: flex; flex-direction: column'><span>Label</span>
            <div style='flex: 1; display: flex; align-items: flex-end'><i class=col style='width: 4px; height: 25%'></i></div>
          </div>
        </div>";
    let found = boxes(html, &["bar", "col", "row"]);
    assert!((found[0].3 - 60.0).abs() < 0.1, "{found:?}");
    assert!((found[0].1 - found[2].1 - 30.0).abs() < 0.1, "{found:?}");
    assert!((found[1].3 - 26.0).abs() < 0.1, "{found:?}");
    assert!((found[1].1 - found[2].1 - 94.0).abs() < 0.1, "{found:?}");
}

#[test]
fn keeps_the_last_margin_inside_a_box_its_minimum_height_raises() {
    // The parent's minimum height makes it taller than its content: the child's bottom margin
    // neither grows the parent nor separates what follows. Where the content is taller, the
    // margin collapses through as without a minimum (WPT margin-collapse-min-height-001, -003).
    let footer_top = |min_height: &str| {
        let (document, pages) = render_document(&format!(
            "<style>@page {{ size: 600px 800px; margin: 0 }} body {{ margin: 0 }}
             .p {{ min-height: {min_height} }} .c {{ height: 30px; margin-bottom: 550px }}
             .f {{ height: 50px }}</style>
             <div class=p><div class=c></div></div><div class=f></div>"
        ));
        let (_, y, _) = find(&pages[0], &document, "f").expect("footer");
        let (_, _, parent) = find(&pages[0], &document, "p").expect("parent");
        (parent.rect.height.to_px(), y)
    };
    assert_eq!(footer_top("100px"), (100.0, 100.0));
    assert_eq!(footer_top("5px"), (30.0, 580.0));
    assert_eq!(footer_top("0"), (30.0, 580.0));
}

#[test]
fn places_new_formatting_contexts_with_auto_margins_beside_floats() {
    // A box that avoids floats fits beside them by its border box: auto margins need no room,
    // and then align it in the room left (WPT css/CSS2/floats/floats-wrap-top-below-bfc-001l).
    // One of a fixed height needs the room all down: the second goes beside the wider float
    // that starts within its height.
    let (document, pages) = render_document(
        "<style>@page { size: 600px 800px; margin: 0 } body { margin: 0 }
         .c { width: 400px } .f { float: left; clear: left; height: 75px }
         .b { display: block; overflow: hidden; width: 200px; height: 50px }</style>
         <div class=c><div class=f style='width: 50px'></div><div class=f style='width: 100px'></div>
         <div class='b one' style='margin-right: auto'></div><div class='b two' style='margin-right: auto'></div></div>
         <div style='clear: both; width: 400px'><div class=f style='width: 50px'></div>
         <div class='b three' style='margin: 0 auto'></div></div>",
    );
    let at = |class: &str| {
        let (x, y, _) = find(&pages[0], &document, class).expect(class);
        (x, y)
    };
    assert_eq!(at("one"), (50.0, 0.0));
    assert_eq!(at("two"), (100.0, 50.0));
    // Centered in the 350 pixels beside the float.
    assert_eq!(at("three"), (125.0, 150.0));
}

#[test]
fn resolves_percentage_heights_of_floats_and_inline_blocks() {
    // Against the definite height of the block their line is in, as in Chrome (WPT
    // css/CSS2/normal-flow/block-formatting-contexts-008); without one they are auto.
    let (document, pages) = render_document(
        "<!doctype html><style>@page { size: 600px 800px; margin: 0 } body { margin: 0; font: 16px/20px Inter }
         .c { width: 200px } .f { float: right; width: 100%; height: 50% }
         .i { display: inline-block; width: 50px; height: 25% }</style>
         <div class=c style='height: 200px'><div class='f one'></div></div>
         <div class=c style='height: 200px'><span class='i two'></span></div>
         <div class=c><span class='i three'>x</span></div>",
    );
    let height = |class: &str| {
        let (_, _, fragment) = find(&pages[0], &document, class).expect(class);
        fragment.rect.height.to_px()
    };
    assert_eq!(height("one"), 100.0);
    assert_eq!(height("two"), 50.0);
    assert_eq!(height("three"), 20.0);
}

#[test]
fn grid_items_across_flexible_tracks_have_no_automatic_minimum() {
    // An item of `width: auto` spanning `auto 1fr` does not widen the flexible column for its
    // wide content (its automatic minimum width is zero); one of `width: 400px` does.
    let html = "<div style='display: grid; width: 200px; grid-template-columns: auto 1fr'>
           <div style='width: 50px; height: 20px'></div><div class=c1 style='grid-column: 2'></div>
           <div style='grid-column: 1 / 3'><div style='width: 400px; height: 10px'></div></div>
         </div>
         <div style='display: grid; width: 200px; grid-template-columns: auto 1fr'>
           <div style='width: 50px; height: 20px'></div><div class=c2 style='grid-column: 2'></div>
           <div style='grid-column: 1 / 3; width: 400px; height: 10px'></div>
         </div>";
    assert_eq!(widths(html, &["c1", "c2"]), [150.0, 350.0]);
    // Rows `auto 1fr auto` in a grid 300px tall: a recipe card's side column spanning the last
    // two rows overflows them instead of growing the flexible row, so the tip in the last row
    // stays at the bottom of the grid, as in Chrome; with a fixed height it grows the row.
    let rows = |side: &str| {
        format!(
            "<div style='display: grid; height: 300px; grid-template-columns: 100px 100px;
                         grid-template-rows: auto 1fr auto'>
               <div style='grid-column: 1 / -1; height: 20px'></div>
               <div class=side style='grid-row: 2 / 4; {side}'><div style='height: 330px'></div></div>
               <div style='height: 50px'></div>
               <div class=tip style='grid-column: 2; height: 40px'></div>
             </div>"
        )
    };
    let found = boxes(&rows(""), &["side", "tip"]);
    assert_eq!((found[0].3, found[1].1 - found[0].1), (280.0, 240.0));
    let found = boxes(&rows("height: 330px"), &["side", "tip"]);
    assert_eq!((found[0].3, found[1].1 - found[0].1), (330.0, 290.0));
}

#[test]
fn sizes_tables_but_not_their_cells_by_aspect_ratio() {
    let found = boxes(
        "<style>table { border-collapse: collapse } td { padding: 0 }</style>
         <table><tr><td class=cell style='width: 100px; aspect-ratio: 1'></td>
           <td class=short style='height: 50px; aspect-ratio: 4'></td></tr></table>
         <table class=table style='width: 100px; aspect-ratio: 2 / 1'></table>",
        &["cell", "short", "table"],
    );
    // Internal table boxes take no ratio (WPT `aspect-ratio/table-element-001`): the row is
    // as tall as the second cell, which is as wide as nothing makes it.
    assert_eq!(found[0].3, 50.0, "{found:?}");
    assert!(found[1].2 < 10.0, "{found:?}");
    // The table takes the height of its width.
    assert_eq!((found[2].2, found[2].3), (100.0, 50.0), "{found:?}");
}

#[test]
fn contributes_widths_through_calc_size() {
    // WPT `calc-size/calc-size-min-max-sizes-001`: the min-content and max-content
    // contributions of `calc-size(auto, size + 80px)` are those of its content plus 80px.
    let found = widths(
        "<div style='width: 0'><div class=fit style='width: fit-content'>
           <div style='width: calc-size(auto, size + 80px)'><span style='display: inline-block; width: 20px'></span><span
             style='display: inline-block; width: 20px'></span></div></div></div>
         <div style='width: 500px'><div class=wide style='width: fit-content'>
           <div style='width: calc-size(auto, size + 80px)'><span style='display: inline-block; width: 20px'></span><span
             style='display: inline-block; width: 20px'></span></div></div></div>",
        &["fit", "wide"],
    );
    assert_eq!(found, vec![100.0, 120.0]);
}

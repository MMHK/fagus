//! Columns as Chrome prints them: rows of columns of a `column-height`, spanners in them, rules
//! below the content, heights from `min-height` and flex items, breaks avoided between columns,
//! nested columns measured unbroken, lines trimmed at the ends of columns, floats that go on
//! before a spanner, fieldsets in columns, rows of a group that avoids breaks inside at the
//! top of a page, column flex containers and floats that clear floats going on to the next
//! column. The numbers are Chrome's (WPT css-multicol, text-box-trim and CSS2 pagination
//! tests).

use super::render_document;
use crate::fragment::{Fragment, FragmentKind, Page};

/// Renders `body` on pages 400px × 300px without margins.
fn render(css: &str, body: &str) -> (fagus_dom::Document, Vec<Page>) {
    render_document(&format!(
        "<style>@page {{ size: 400px 300px; margin: 0 }} body {{ margin: 0; font: 16px/20px Inter }}
         p {{ margin: 0 }} {css}</style>{body}"
    ))
}

/// The fragments of the elements with class `class` on `page`, with their places on the page.
fn found<'p>(document: &fagus_dom::Document, page: &'p Page, class: &str) -> Vec<((f32, f32), &'p Fragment)> {
    fn walk<'p>(
        fragment: &'p Fragment,
        document: &fagus_dom::Document,
        class: &str,
        origin: (f32, f32),
        out: &mut Vec<((f32, f32), &'p Fragment)>,
    ) {
        let x = origin.0 + fragment.rect.x.to_px();
        let y = origin.1 + fragment.rect.y.to_px();
        if let FragmentKind::Box { node: Some(node), .. } = &fragment.kind
            && document
                .element(*node)
                .is_some_and(|data| data.classes.iter().any(|name| &**name == class))
        {
            out.push(((x, y), fragment));
        }
        for child in &fragment.children {
            walk(child, document, class, (x, y), out);
        }
    }
    let mut out = Vec::new();
    for fragment in &page.fragments {
        walk(fragment, document, class, (0.0, 0.0), &mut out);
    }
    out
}

/// The border boxes (x, y, width, height) of the fragments of the elements with class `class`
/// on `page`, in page coordinates.
fn boxes(document: &fagus_dom::Document, page: &Page, class: &str) -> Vec<(f32, f32, f32, f32)> {
    found(document, page, class)
        .into_iter()
        .map(|((x, y), fragment)| (x, y, fragment.rect.width.to_px(), fragment.rect.height.to_px()))
        .collect()
}

/// How many lines lie below `fragment`.
fn lines(fragment: &Fragment) -> usize {
    fragment
        .children
        .iter()
        .map(|child| usize::from(matches!(child.kind, FragmentKind::Line { .. })) + lines(child))
        .sum()
}

#[test]
fn makes_rows_of_columns_from_column_height_alone() {
    // `column-height` makes a multi-column container of one column: the content 80px tall
    // takes two rows 40px tall, 20px apart.
    let (document, pages) = render(
        ".mc { column-height: 40px; row-gap: 20px; width: 100px } .b { height: 80px }",
        "<div class=mc><div class=b></div></div>",
    );
    assert_eq!(
        boxes(&document, &pages[0], "b"),
        [(0.0, 0.0, 100.0, 40.0), (0.0, 60.0, 100.0, 40.0)]
    );
    assert_eq!(boxes(&document, &pages[0], "mc")[0].3, 100.0);
}

#[test]
fn gives_the_last_row_all_of_column_height() {
    // Eight blocks, each ending a column, in rows of two columns 25px tall: the last row is
    // as tall as the others, though its content is 1px tall.
    let (document, pages) = render(
        ".mc { columns: 2; column-height: 25px; column-wrap: wrap; gap: 0; width: 100px }
         .b { height: 1px; break-after: column }",
        &format!("<div class=mc>{}</div>", "<div class=b></div>".repeat(8)),
    );
    assert_eq!(boxes(&document, &pages[0], "mc")[0].3, 100.0);
}

#[test]
fn moves_spanners_that_do_not_fit_in_the_rest_of_a_row_to_the_next() {
    // Rows 20px tall, 10px apart: the first row is balanced to 10px, the spanner 25px tall does
    // not fit in the 10px left of it and starts the next row (at 30px), and the content after
    // it continues in the row after that one (at 60px).
    let (document, pages) = render(
        ".mc { columns: 2; column-height: 20px; column-wrap: wrap; gap: 10px 0; width: 100px }
         .a { height: 20px } .span { column-span: all; height: 25px } .b { height: 60px }",
        "<div class=mc><div class=a></div><div class=span></div><div class=b></div></div>",
    );
    assert_eq!(boxes(&document, &pages[0], "span"), [(0.0, 30.0, 100.0, 25.0)]);
    assert_eq!(
        boxes(&document, &pages[0], "b"),
        [
            (0.0, 60.0, 50.0, 20.0),
            (50.0, 60.0, 50.0, 20.0),
            (0.0, 90.0, 50.0, 10.0),
            (50.0, 90.0, 50.0, 10.0)
        ]
    );
    // The last row takes all of `column-height`.
    assert_eq!(boxes(&document, &pages[0], "mc")[0].3, 110.0);
}

#[test]
fn paints_column_rules_below_the_content_of_the_columns() {
    // The rules come before the columns among the container's children, so content that
    // overflows its column paints over them, as in Chrome.
    let (document, pages) = render(
        ".mc { columns: 3; column-gap: 30px; column-rule: 30px solid; width: 180px; height: 100px }",
        "<div class=mc><div style='height: 250px'></div></div>",
    );
    let (_, mc) = found(&document, &pages[0], "mc")[0];
    let first_column = mc
        .children
        .iter()
        .position(|child| (child.rect.width.to_px() - 40.0).abs() < 0.01)
        .expect("a column");
    let first_rule = mc
        .children
        .iter()
        .position(|child| (child.rect.width.to_px() - 30.0).abs() < 0.01)
        .expect("a rule");
    assert!(first_rule < first_column);
}

#[test]
fn bounds_balanced_columns_by_min_height_over_height_and_max_height() {
    // `height: 20px` and `max-height: 40px` lose to `min-height: 100px`: the columns are 100px
    // tall and hold the content in two.
    let (document, pages) = render(
        ".mc { columns: 2; column-gap: 0; width: 100px; height: 20px; max-height: 40px; min-height: 100px }
         .b { height: 200px }",
        "<div class=mc><div class=b></div></div>",
    );
    assert_eq!(
        boxes(&document, &pages[0], "b"),
        [(0.0, 0.0, 50.0, 100.0), (50.0, 0.0, 50.0, 100.0)]
    );
}

#[test]
fn avoids_breaks_inside_boxes_between_columns_only_with_avoid_column() {
    // Boxes 200px tall in columns 300px tall: with `avoid-column` each starts a column (the
    // last one added beside the container), with `avoid-page` the second breaks across columns.
    let css = ".mc { columns: 2; column-gap: 0; column-fill: auto; width: 200px; height: 300px }
               .b { height: 200px }";
    let (document, pages) = render(
        css,
        "<div class=mc><div class=b style='break-inside: avoid-column'></div>
         <div class=b style='break-inside: avoid-column'></div><div class=b style='break-inside: avoid-column'></div></div>",
    );
    assert_eq!(
        boxes(&document, &pages[0], "b"),
        [
            (0.0, 0.0, 100.0, 200.0),
            (100.0, 0.0, 100.0, 200.0),
            (200.0, 0.0, 100.0, 200.0)
        ]
    );
    let (document, pages) = render(
        css,
        "<div class=mc><div class=b style='break-inside: avoid-page'></div>
         <div class=b style='break-inside: avoid-page'></div></div>",
    );
    assert_eq!(
        boxes(&document, &pages[0], "b"),
        [
            (0.0, 0.0, 100.0, 200.0),
            (0.0, 200.0, 100.0, 100.0),
            (100.0, 0.0, 100.0, 100.0)
        ]
    );
}

#[test]
fn fills_the_height_of_stretched_flex_items_with_columns() {
    // The flex item is stretched to 100px: its one column holds the first box, which avoids
    // breaks inside, and the second goes in a column added beside it.
    let (document, pages) = render(
        ".flex { display: flex; width: 100px; height: 100px } .mc { columns: 1; width: 50px; gap: 0 }
         .b { break-inside: avoid; height: 100px }",
        "<div class=flex><div class=mc><div class=b></div><div class=b></div></div></div>",
    );
    assert_eq!(
        boxes(&document, &pages[0], "b"),
        [(0.0, 0.0, 50.0, 100.0), (50.0, 0.0, 50.0, 100.0)]
    );
}

#[test]
fn balances_columns_around_columns_that_fill_in_turn() {
    // The inner columns (`column-fill: auto`) are measured unbroken, balanced: the content
    // 1000px tall takes ten columns 100px tall, two in each of the five outer ones.
    let (document, pages) = render(
        ".outer { columns: 5; gap: 0; width: 100px } .inner { columns: 2; column-fill: auto; gap: 0 }
         .b { height: 1000px }",
        "<div class=outer><div class=inner><div class=b></div></div></div>",
    );
    assert_eq!(boxes(&document, &pages[0], "outer")[0].3, 100.0);
}

#[test]
fn trims_lines_at_the_start_and_end_of_each_column() {
    // Lines 40px tall in columns 150px tall: trimmed at the top of each column and, where only
    // that makes it fit, at the bottom, four lines fit in each column instead of three.
    let (document, pages) = render(
        ".mc { columns: 2; column-fill: auto; column-gap: 0; width: 200px; height: 150px;
           font: 20px/40px Inter; orphans: 1; widows: 1; text-box-trim: trim-both; text-box-edge: text }",
        "<div class=mc>a<br>b<br>c<br>d<br>e<br>f<br>g<br>h</div>",
    );
    let (_, mc) = found(&document, &pages[0], "mc")[0];
    let columns: Vec<usize> = mc.children.iter().map(lines).collect();
    assert_eq!(columns, [4, 4]);
    // The first line of the second column starts above its top, by the half-leading trimmed.
    let first = &mc.children[1].children[0];
    assert!(first.rect.y.to_px() < -5.0, "{:?}", first.rect);
}

#[test]
fn continues_floats_in_the_next_column_before_a_spanner() {
    // The float 180px tall is broken across the two columns of the row above the spanner,
    // balanced to 90px, and the spanner follows below.
    let (document, pages) = render(
        ".mc { columns: 2; width: 100px; gap: 0; orphans: 1; widows: 1 }
         .f { float: left; width: 50px; height: 180px } .span { column-span: all; height: 10px }",
        "<div class=mc><div class=f></div><div><div class=span></div></div></div>",
    );
    assert_eq!(
        boxes(&document, &pages[0], "f"),
        [(0.0, 0.0, 50.0, 90.0), (50.0, 0.0, 50.0, 90.0)]
    );
    assert_eq!(boxes(&document, &pages[0], "span"), [(0.0, 90.0, 100.0, 10.0)]);
}

#[test]
fn lays_out_the_content_of_fieldsets_below_the_legend_in_columns() {
    // The legend stays on the border; the blocks below it go in the two columns.
    let (document, pages) = render(
        ".mc { columns: 2; column-gap: 0; width: 200px; border: 0; padding: 0; margin: 0 }
         .b { height: 20px; break-after: column }",
        "<fieldset class=mc><legend class=lg>Legend</legend><div class=b></div><div class=b></div></fieldset>",
    );
    let legend = boxes(&document, &pages[0], "lg");
    let blocks = boxes(&document, &pages[0], "b");
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].1, blocks[1].1);
    assert_eq!(blocks[1].0 - blocks[0].0, 100.0);
    assert!(legend[0].1 + legend[0].3 <= blocks[0].1);
}

#[test]
fn breaks_rows_of_a_group_that_avoids_breaks_inside_where_it_starts_a_page() {
    // The group moves to the second page, where it does not fit either: its second row breaks
    // inside there, as in Chrome, instead of moving on to a third page.
    let (document, pages) = render(
        "table { border-spacing: 0 } td { padding: 0 } .g { break-inside: avoid } .p { height: 120px }",
        "<div style='height: 100px'></div><table><tbody class=g><tr><td><div class=p></div></td></tr>
         <tr><td><div class=p></div><div class=p></div></td></tr></tbody></table>",
    );
    assert_eq!(pages.len(), 3);
    assert_eq!(boxes(&document, &pages[1], "p").len(), 3);
}

#[test]
fn breaks_absolute_boxes_across_vertical_columns() {
    // In `vertical-rl` the columns lie below one another, 50px tall and 100px wide: the box
    // 200px wide in the block direction fills both.
    let (document, pages) = render(
        ".mc { writing-mode: vertical-rl; columns: 2; column-fill: auto; column-gap: 0; height: 100px; width: 100px }
         .abs { position: absolute; inline-size: 50px; block-size: 200px; inset-block-start: 0; inset-inline-start: 0 }",
        "<div class=mc><div style='position: relative'>x<div class=abs></div></div></div>",
    );
    assert_eq!(
        boxes(&document, &pages[0], "abs"),
        [(0.0, 0.0, 100.0, 50.0), (0.0, 50.0, 100.0, 50.0)]
    );
}

#[test]
fn makes_progress_with_cloned_decorations_taller_than_the_columns() {
    // The box's cloned border and padding (10px) are taller than the columns (7px): its
    // content stays in the first column and its decorations end in the second, as in Chrome,
    // instead of repeating in columns added without end.
    let (document, pages) = render(
        ".mc { columns: 2; width: 200px; height: 7px }
         .b { box-decoration-break: clone; padding: 4px 0; border: 1px solid; height: 2px }",
        "<div class=mc><div class=b></div></div>",
    );
    assert_eq!(pages.len(), 1);
    assert_eq!(boxes(&document, &pages[0], "b").len(), 2);
}

#[test]
fn keeps_the_margins_of_column_flex_items_across_columns() {
    // A column flex container of automatic height breaks as Blink's does: its items keep their
    // places in the unbroken column, margins included. The first item's top margin fills the
    // rest of the first column (the item starts the second), the second item's 150px margin
    // takes the third column and half the fourth (WPT css-break single-line-column-flex-
    // fragmentation-033).
    let (document, pages) = render(
        ".mc { columns: 5; column-fill: auto; gap: 0; width: 100px; height: 100px }
         .flex { display: flex; flex-direction: column } .flex > div { width: 20px }
         .a { height: 100px; margin-top: 50px } .b { height: 150px; margin-top: 150px }",
        "<div class=mc><div style='height: 50px'></div>
         <div class=flex><div class=a></div><div class=b></div></div></div>",
    );
    assert_eq!(boxes(&document, &pages[0], "a"), [(20.0, 0.0, 20.0, 100.0)]);
    assert_eq!(
        boxes(&document, &pages[0], "b"),
        [(60.0, 50.0, 20.0, 50.0), (80.0, 0.0, 20.0, 100.0)]
    );
    // The container goes on as long as its items do.
    assert_eq!(boxes(&document, &pages[0], "flex").len(), 5);
}

#[test]
fn breaks_column_flex_containers_before_their_last_item_for_their_bottom_border() {
    // The bottom border (40px) does not fit below the second item: the container breaks before
    // that item, which goes to the next column with the border, as in Chrome (WPT css-break
    // single-line-column-flex-fragmentation-023).
    let (document, pages) = render(
        ".mc { columns: 2; column-fill: auto; gap: 0; width: 100px; height: 160px }
         .flex { display: flex; flex-direction: column; width: 50px; border-bottom: 40px solid }
         .i { break-inside: avoid; height: 60px }",
        "<div class=mc><div style='height: 40px'></div>
         <div class=flex><div class=i></div><div class='i last'></div></div></div>",
    );
    assert_eq!(boxes(&document, &pages[0], "last"), [(50.0, 0.0, 50.0, 60.0)]);
    assert_eq!(boxes(&document, &pages[0], "flex")[1], (50.0, 0.0, 50.0, 100.0));
}

#[test]
fn grows_column_flex_items_whose_flex_basis_is_their_content() {
    // The item's height is its content's (`flex-basis: content`, 150px); the content's second
    // block does not fit beside the first and moves to the second column, where the item grows
    // to hold it (WPT css-break single-line-column-flex-fragmentation-050).
    let (document, pages) = render(
        ".mc { columns: 2; gap: 0; width: 100px; height: 100px }
         .flex { display: flex; flex-direction: column }
         .item { flex-basis: content; height: 10px } .c { contain: size; width: 50px }",
        "<div class=mc><div class=flex><div class=item>
         <div class=c style='height: 50px'></div><div class=c style='height: 100px'></div>
         </div></div></div>",
    );
    assert_eq!(
        boxes(&document, &pages[0], "item"),
        [(0.0, 0.0, 50.0, 100.0), (50.0, 0.0, 50.0, 100.0)]
    );
}

#[test]
fn moves_floats_that_clear_a_float_going_on_to_the_next_column() {
    // The first float goes on in the second column: the empty float that clears it starts
    // there, below its rest, not at the end of the first column (WPT css-break float-001).
    let (document, pages) = render(
        ".mc { columns: 2; column-fill: auto; gap: 0; width: 100px; height: 100px }
         .f { float: left; width: 100% } .tall { height: 200px } .after { clear: left; height: 0 }",
        "<div class=mc><div class='f tall'></div><div class='f after'></div></div>",
    );
    assert_eq!(boxes(&document, &pages[0], "after"), [(50.0, 100.0, 50.0, 0.0)]);
}

#[test]
fn balances_columns_without_the_margin_that_ends_a_formatting_context_in_them() {
    // The bottom margin of the last child inside a formatting context in the columns is
    // truncated at the end of the column, as Blink truncates it where the fragmentainer ends
    // (`AdjustedMarginAfterFinalChildFragment`): the balanced columns do not grow for it
    // (WPT css-multicol multicol-span-all-010; Chrome 157 prints these heights).
    let (document, pages) = render(
        ".mc { columns: 2; gap: 0; width: 200px } .bfc { display: flow-root }
         .bfc p { margin: 15px 0 30px }",
        "<div class='mc one'><div class=bfc><p>Alone</p></div></div>
         <div class='mc three'><div class=bfc><p>A</p><p>B</p><p>C</p></div></div>",
    );
    assert_eq!(boxes(&document, &pages[0], "one"), [(0.0, 0.0, 200.0, 35.0)]);
    // An even share of the content with the margin (165px), at which the columns hold it.
    assert_eq!(boxes(&document, &pages[0], "three"), [(0.0, 35.0, 200.0, 82.5)]);
}

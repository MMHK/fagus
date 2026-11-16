//! Boxes beside floats and margins that collapse through empty boxes, as Chrome lays them out
//! (from the Web Platform Tests in CSS2/floats-clear and CSS2/floats).

use super::{find, render_document};

fn page(html: &str) -> String {
    format!(
        "<!DOCTYPE html><style>@page {{ size: 600px 400px; margin: 0 }} body {{ margin: 0; font: 12px/16px Inter }}</style>{html}"
    )
}

fn layout_box(html: &str, class: &str) -> (f32, f32, f32, f32) {
    let (document, pages) = render_document(&page(html));
    let (x, y, fragment) = find(&pages[0], &document, class).expect(class);
    (x, y, fragment.rect.width.to_px(), fragment.rect.height.to_px())
}

#[test]
fn moves_new_formatting_contexts_to_room_for_their_height() {
    // Beside the first float the box would be 200 wide and 50 tall, overlapping the second;
    // it goes beside both instead, 100 wide, where it is 100 tall (floats-bfc-003).
    let placed = layout_box(
        "<div style='width: 300px'>
           <div style='float: left; clear: left; width: 100px; height: 25px'></div>
           <div style='float: left; clear: left; width: 200px; height: 25px'></div>
           <div class=b style='overflow: hidden'>
             <div style='float: left; width: 100px; height: 50px'></div>
             <div style='float: left; width: 100px; height: 50px'></div></div></div>",
        "b",
    );
    assert_eq!(placed, (200.0, 0.0, 100.0, 100.0));
}

#[test]
fn collapses_margins_through_empty_boxes_into_their_parent() {
    // The margin of the box after an empty one collapses through it with its parent's: the
    // parent starts that far down, past the float, so the box needs no clearance and the parent
    // stays empty (no-clearance-adjoining-opposite-float).
    let (document, pages) = render_document(&page(
        "<div style='height: 20px'></div><div style='float: left; width: 10px; height: 100px'></div>
         <div class=o><div><div style='float: right; width: 10px; height: 200px'></div></div>
           <div class=r><div style='margin-top: 150px; clear: left'></div></div></div>",
    ));
    let (_, y, red) = find(&pages[0], &document, "r").expect("box");
    assert_eq!(red.rect.height.to_px(), 0.0);
    assert_eq!(y, 170.0);
    // A box that does not fit beside a float inside an empty box keeps its margin to itself:
    // the parent does not move down with it (new-fc-separates-from-float).
    let parent = layout_box(
        "<div style='overflow: hidden; width: 200px'><div class=o><div><div style='float: left; width: 200px; height: 200px'></div></div>
           <div style='margin-top: 200px; overflow: hidden; width: 200px; height: 1px'></div></div></div>",
        "o",
    );
    assert_eq!(parent.1, 0.0);
}

#[test]
fn takes_floats_back_that_follow_where_the_line_ends() {
    // The float follows unbreakable content that overflows the first line, which therefore ends
    // before it: it is met again on the second line, after that content, where it does not fit
    // and goes below that line, which stays where it is (float-nowrap-3).
    let html = "<div style='width: 100px'><span style='display: inline-block; width: 40px; height: 10px'></span>
         <span style='white-space: nowrap'><span class=b style='display: inline-block; width: 70px; height: 10px'></span><span
         class=f style='float: right; width: 50px; height: 30px'></span>x</span></div>";
    let (document, pages) = render_document(&page(html));
    let (_, line, _) = find(&pages[0], &document, "b").expect("box");
    let (_, float, _) = find(&pages[0], &document, "f").expect("float");
    assert!(line > 0.0 && line < float, "line {line}, float {float}");
    // Where it fits beside what comes before it on the second line, it stands at that line's
    // top and the line stays beside it, overflowing (float-nowrap-9).
    let html = "<div style='width: 100px'><span style='display: inline-block; width: 40px; height: 10px'></span>
         <span style='white-space: nowrap'><span class=b style='display: inline-block; width: 20px; height: 10px'></span><span
         class=f style='float: right; width: 50px; height: 30px'></span><span style='display: inline-block; width: 70px; height: 10px'></span></span></div>";
    let (document, pages) = render_document(&page(html));
    let (_, line, _) = find(&pages[0], &document, "b").expect("box");
    let (x, float, _) = find(&pages[0], &document, "f").expect("float");
    assert_eq!((x, float), (50.0, 16.0));
    assert!(line > 0.0 && line < float + 30.0, "line {line}, float {float}");
}

#[test]
fn places_floats_that_start_a_line_even_where_they_are_wider_than_it() {
    // A float before anything of its line goes at the line's top however wide it is, and the
    // content after it below it (float-wider-than-line-inside-inline-with-block).
    let (document, pages) = render_document(&page(
        "<div style='width: 40px'><span class=f style='float: left; width: 100px; height: 20px'></span><span
         class=b style='display: inline-block; width: 10px; height: 10px'></span></div>",
    ));
    let (_, float, _) = find(&pages[0], &document, "f").expect("float");
    let (_, line, _) = find(&pages[0], &document, "b").expect("box");
    assert_eq!(float, 0.0);
    assert!(line >= 20.0, "{line}");
}

#[test]
fn moves_the_line_after_floats_that_fill_the_page_beside_the_float_on_the_next() {
    // Four floats fill the first page; the fifth goes to the top of the next, and the line
    // after them goes there too, beside it, the floats staying on the first page (WPT css-page
    // page-size-007; Chrome 157, measured).
    let floats = "<div style='float: left; width: 100px; height: 80px'></div>".repeat(5);
    let (document, pages) = render_document(&format!(
        "<!DOCTYPE html><style>@page {{ size: 200px 160px; margin: 0 }} body {{ margin: 0; font: 16px Inter }}</style>
         <div style='display: flow-root'>{floats}<span class=s>second</span></div>"
    ));
    assert_eq!(pages.len(), 2);
    let (x, y, _) = find(&pages[1], &document, "s").expect("line on the second page");
    assert_eq!((x, y), (100.0, 0.0));
}

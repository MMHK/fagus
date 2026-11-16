//! Auto margins and alignment of flex items as Chrome does them: auto margins take the free room
//! in either axis before `justify-content` and `align-self`, and items of a column with a width
//! of their own are aligned across it. The numbers are Chrome's.

use super::{find, render_document};

/// The x and y positions of the boxes with the given classes, in CSS px.
fn positions(html: &str, classes: &[&str]) -> Vec<(f32, f32)> {
    let (document, pages) = render_document(&format!(
        "<style>@page {{ size: 600px 800px; margin: 0 }} body {{ margin: 0; font: 12px/16px Inter }}
         i {{ display: block; width: 50px; height: 10px }}</style>{html}"
    ));
    classes
        .iter()
        .map(|class| {
            let (x, y, _) = find(&pages[0], &document, class).expect(class);
            (x, y)
        })
        .collect()
}

#[test]
fn gives_free_room_in_a_row_to_auto_margins() {
    let found = positions(
        "<div style='display: flex; width: 400px'><i class=a></i><i class=b style='margin-left: auto'></i></div>
         <div style='display: flex; width: 400px; justify-content: center'><i class=c style='margin: 0 auto'></i><i class=d></i></div>
         <div style='display: flex; width: 400px; flex-direction: row-reverse'><i class=e></i><i class=f style='margin-right: auto'></i></div>
         <div style='display: flex; width: 400px; height: 50px'><i class=g style='margin: auto'></i></div>",
        &["a", "b", "c", "d", "e", "f", "g"],
    );
    assert_eq!(found[0].0, 0.0);
    assert_eq!(found[1].0, 350.0);
    // Both auto margins of the first item share the room; justify-content has none left.
    assert_eq!((found[2].0, found[3].0), (150.0, 350.0));
    assert_eq!((found[4].0, found[5].0), (350.0, 0.0));
    // An item with auto margins in both axes is centred and not stretched.
    assert_eq!(found[6], (175.0, 30.0 + 20.0));
}

#[test]
fn aligns_items_of_a_column_across_it() {
    let found = positions(
        "<div style='display: flex; flex-direction: column; align-items: center; width: 400px'><i class=a></i><i class=b style='margin-left: 20px'></i></div>
         <div style='display: flex; flex-direction: column; align-items: flex-end; width: 400px'><i class=c></i></div>
         <div style='display: flex; flex-direction: column; width: 400px'><i class=d style='margin-left: auto'></i><b class=e style='margin: 0 auto'>x</b></div>",
        &["a", "b", "c", "d", "e"],
    );
    assert_eq!(found[0].0, 175.0);
    assert_eq!(found[1].0, 185.0);
    assert_eq!(found[2].0, 350.0);
    assert_eq!(found[3].0, 350.0);
    // An auto width with auto margins shrinks to its content and is centred.
    assert!(found[4].0 > 190.0 && found[4].0 < 200.0, "{found:?}");
}

#[test]
fn gives_free_room_in_a_column_to_auto_margins() {
    let found = positions(
        "<div style='display: flex; flex-direction: column; height: 100px'><i class=a></i><i class=b style='margin-top: auto'></i></div>
         <div style='position: relative; height: 200px'>
           <div style='position: absolute; inset: 0; display: flex; flex-direction: column'><i class=c></i><i class=d style='margin-top: auto'></i></div>
         </div>
         <div style='display: flex; height: 100px'>
           <div style='display: flex; flex-direction: column'><i class=e></i><i class=f style='margin-top: auto'></i></div>
         </div>",
        &["a", "b", "c", "d", "e", "f"],
    );
    assert_eq!(found[0].1, 0.0);
    assert_eq!(found[1].1, 90.0);
    // Between the insets of an absolutely positioned box, and stretched in a row.
    assert_eq!(found[3].1, 100.0 + 190.0);
    assert_eq!(found[5].1, 300.0 + 90.0);
}

#[test]
fn aligns_items_by_their_last_baselines_at_the_end() {
    let found = positions(
        "<style>.f { display: flex; height: 100px; align-items: last baseline }
           .g { display: grid; grid-template-columns: repeat(3, auto); grid-template-rows: 100px;
                align-items: last baseline }
           .big { font-size: 30px; line-height: 36px } .pad { padding-bottom: 10px }</style>
         <div class=f><div class=a>one<br>two</div><div class='b big'>Big</div><div class='c pad'>pad</div></div>
         <div class=g><div class=d>one<br>two</div><div class='e big'>Big</div><div class='h pad'>pad</div></div>",
        &["a", "b", "c", "d", "e", "h"],
    );
    // The last baselines meet where the padded item's room below its own puts them, at the
    // end of the line or row (Chrome 157).
    let tops: Vec<f32> = found.iter().map(|position| position.1).collect();
    assert_eq!(tops, [58.0, 57.0, 74.0, 158.0, 157.0, 174.0]);
}

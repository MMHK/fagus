//! Boxes that are containing blocks or paint absolutely positioned boxes without being blocks
//! of their own in the flow: stacking contexts inside the containing block, positioned table
//! rows and row groups, relatively positioned inline boxes around blocks or around nothing but
//! such boxes. The numbers are Chrome's (157), measured with the same documents.

use super::{find, render_document};
use crate::fragment::{Fragment, FragmentKind};

/// Renders `body` on pages 400px × 400px without margins.
fn render(css: &str, body: &str) -> (fagus_dom::Document, Vec<crate::fragment::Page>) {
    render_document(&format!(
        "<style>@page {{ size: 400px 400px; margin: 0 }} body {{ margin: 0; font: 16px/20px Inter }} {css}</style>{body}"
    ))
}

/// Whether a box with class `class` lies among the descendants of `fragment`.
fn holds(fragment: &Fragment, document: &fagus_dom::Document, class: &str) -> bool {
    fragment.children.iter().any(|child| {
        matches!(&child.kind, FragmentKind::Box { node: Some(node), .. }
            if document.element(*node).is_some_and(|data| data.classes.iter().any(|name| &**name == class)))
            || holds(child, document, class)
    })
}

#[test]
fn paints_absolute_boxes_in_the_stacking_context_around_them() {
    // The box's containing block is the page, but the translucent box around it paints it, in
    // its group (WPT css-break abspos-in-opacity-000).
    let (document, pages) = render(
        ".a { position: absolute; width: 100px; height: 100px }",
        "<div style='height: 30px'></div><div class=o style='opacity: 0.5'><div class=a></div></div>",
    );
    let (_, _, faded) = find(&pages[0], &document, "o").expect("translucent box");
    assert!(holds(faded, &document, "a"));
    let (x, y, _) = find(&pages[0], &document, "a").expect("positioned box");
    assert_eq!((x, y), (0.0, 30.0));
    // Not where a box in between clips: it clips the group, not the positioned box.
    let (document, pages) = render(
        ".a { position: absolute; width: 100px; height: 100px }",
        "<div style='overflow: hidden; height: 10px'><div class=o style='opacity: 0.5'><div class=a></div></div></div>",
    );
    let (_, _, faded) = find(&pages[0], &document, "o").expect("translucent box");
    assert!(!holds(faded, &document, "a"));
}

#[test]
fn positioned_rows_and_row_groups_contain_absolute_boxes() {
    // The row group is 200px tall, its first row 120px: the box after that row, in an
    // anonymous cell, is placed below it at 70% of the group's height, as wide as the group
    // (WPT css-break table/oof-static-pos-percentage-height-002).
    let (document, pages) = render(
        ".a { position: absolute; width: 100%; height: 70% }",
        "<div style='margin-left: 50px'><div style='position: relative; display: table-row-group'>
           <div style='display: table-row'><div style='width: 25px; height: 120px'></div></div>
           <div class=a></div>
           <div style='display: table-row'><div style='width: 5px; height: 80px'></div></div>
         </div></div>",
    );
    let (x, y, fragment) = find(&pages[0], &document, "a").expect("positioned box");
    assert_eq!((x, y), (50.0, 120.0));
    assert_eq!(
        (fragment.rect.width.to_px(), fragment.rect.height.to_px()),
        (25.0, 140.0)
    );
    // A relatively positioned row moves its cells by its offsets.
    let (document, pages) = render(
        "td { padding: 0; width: 20px; height: 20px }",
        "<table style='border-spacing: 0'><tr style='position: relative; top: 10px; left: 5px'>
           <td class=c></td></tr></table>",
    );
    let (x, y, _) = find(&pages[0], &document, "c").expect("cell");
    assert_eq!((x, y), (5.0, 10.0));
}

#[test]
fn relative_inline_boxes_without_lines_contain_absolute_boxes() {
    // An inline box that holds only the positioned box is its containing block where it is,
    // below the block before it, moved by its offset (WPT css-break out-of-flow-in-multicolumn-056).
    let (document, pages) = render(
        ".a { position: absolute; width: 50px }",
        "<div style='margin-left: 100px; width: 100px'><div style='height: 50px'></div>
           <span style='position: relative; left: 20px'><div class=a style='top: -40px; height: 30px'></div></span>
         </div>",
    );
    let (x, y, _) = find(&pages[0], &document, "a").expect("positioned box");
    assert_eq!((x, y), (120.0, 10.0));
    // Around a block, it contains the positioned box after the block from the block's top
    // (WPT css-break out-of-flow-in-multicolumn-058).
    let (document, pages) = render(
        ".a { position: absolute; width: 50px }",
        "<div style='margin-left: 100px; width: 100px'>
           <span style='position: relative; top: 10px; left: 100px'>
             <div style='height: 50px; width: 50px'></div><div class=a style='top: 0; height: 50px'></div>
           </span>
         </div>",
    );
    let (x, y, _) = find(&pages[0], &document, "a").expect("positioned box");
    assert_eq!((x, y), (200.0, 10.0));
}

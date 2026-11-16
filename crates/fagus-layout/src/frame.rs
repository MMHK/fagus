//! Documents in frames (`<iframe>`, and `<object>` and `<embed>` showing HTML), as Chrome prints
//! them: each is a document of its own, laid out on a viewport as large as the content box of
//! its frame, as on a screen, without pages. Its initial containing block, its viewport units and
//! the width and height of its media queries are the frame's; its media type is still `print`.
//! What does not fit in the frame is cut off at the frame's edges, at the start of the document
//! (it is not scrolled), and its canvas is transparent unless its root or body has a background
//! (or it is in the dark color scheme), so that the frame's own background shows through
//! (Chrome 157, measured).

use std::sync::Arc;

use fagus_css::RuleSet;
use fagus_dom::Document;
use fagus_style::{ComputedStyle, StyledDocument};
use fagus_text::FontDb;

use crate::block::{Layouter, Space};
use crate::counter_style::CounterStyles;
use crate::fragment::{Children, Fragment, FragmentKind, Page, PageMarks, PageOrientation};
use crate::image::Images;
use crate::tree::{BoxTree, Targets};
use crate::units::{Au, Rect};

/// The document of a frame, laid out in the frame's content box: drawn in place of the blank
/// image that stands for it in layout (see [`Images::insert_frame`]), clipped to the frame.
#[derive(Debug)]
pub struct FrameDocument {
    /// The viewport the document is laid out on, as large as the frame's content box, at its
    /// corner; the content may reach past it.
    pub page: Page,
    /// The images of the document (and the documents of the frames in it).
    pub images: Images,
    /// How tall the content of the document is, where it lets a frame take that height
    /// (`<meta name=responsive-embedded-sizing>` in its head, see `frame-sizing`); `None` where
    /// it does not.
    pub responsive_height: Option<f32>,
}

/// Whether `document` lets the frame it is in take its size (CSS Sizing 4, `frame-sizing`): it
/// has a `<meta name=responsive-embedded-sizing>` in its head, put there by the parser, as
/// Chrome reads it once the document is parsed.
pub fn is_responsive(document: &Document) -> bool {
    let Some(head) = document.document_element().and_then(|root| {
        document
            .child_elements(root)
            .find(|&child| document.element(child).is_some_and(|data| data.is("head")))
    }) else {
        return false;
    };
    document.child_elements(head).any(|child| {
        document.element(child).is_some_and(|data| {
            data.is("meta")
                && data
                    .attribute("name")
                    .is_some_and(|name| name.trim().eq_ignore_ascii_case("responsive-embedded-sizing"))
        })
    })
}

/// The height of the content of a page laid out by [`layout_viewport`]: down to the bottom of
/// the root's box.
pub fn content_height(page: &Page) -> f32 {
    page.fragments
        .first()
        .map_or(0.0, |root| (root.rect.y + root.rect.height).to_px().max(0.0))
}

/// Lays out a styled document on a viewport `width` × `height` pixels (whole ones, as Chrome
/// sizes the viewport of a frame), as the document of a frame: in one piece, without
/// fragmentation and without `@page` rules (see the module documentation).
#[allow(clippy::too_many_arguments)]
pub fn layout_viewport(
    document: &Document,
    styled: &StyledDocument,
    rules: &RuleSet,
    fonts: &FontDb,
    images: &Images,
    locale: Option<&str>,
    (width, height): (f32, f32),
) -> Page {
    let tree = BoxTree::build_with(
        document,
        styled,
        images,
        Targets::default(),
        CounterStyles::new(rules),
        Some(fonts),
        locale,
    );
    let (width, height) = (Au::from_px(width.max(0.0)), Au::from_px(height.max(0.0)));
    let area = Rect::new(Au::ZERO, Au::ZERO, width, height);
    let mut page = Page {
        width,
        height,
        fragments: Vec::new(),
        margin: [Au::ZERO; 4],
        style: Arc::new(ComputedStyle::default()),
        canvas: tree.canvas.clone(),
        bleed: 0.0,
        marks: PageMarks::default(),
        orientation: PageOrientation::Upright,
        scale: 1.0,
        area,
        shift: 0.0,
        carried: Vec::new(),
    };
    let Some(root) = tree.root else {
        return page;
    };
    let layouter = Layouter::new(&tree, fonts);
    // Nothing in the document is fragmented (see `crate::unfragmented`).
    let _whole = layouter.enter_unfragmented();
    layouter.page_number.set((1, 1));
    layouter.initial_containing_block.set((width, height));
    let mode = layouter.writing_mode();
    let (inline, block) = crate::writing::physical_size((width, height), mode);
    let style = &tree.get(root).style;
    let space = Space {
        containing_height: crate::positioned::root_block_room(style, block).or(Some(block)),
        ..Space::unfragmented(crate::positioned::root_inline_room(
            style,
            inline,
            mode.is_vertical(),
        ))
    };
    let mut content = match layouter.block_level(root, space, None) {
        Some(outcome) => {
            let mut content = outcome.fragment;
            // The root's top margin collapses with no other.
            content.rect.y += outcome.collapsed_top.resolve();
            content
        }
        None => Fragment {
            rect: Rect::new(Au::ZERO, Au::ZERO, inline, Au::ZERO),
            kind: FragmentKind::Anonymous,
            children: Children::new(),
        },
    };
    if !mode.is_vertical() {
        let (dx, dy) = crate::positioned::root_offset(style, content.rect, (inline, block), true);
        content.rect.x += dx;
        content.rect.y += dy;
    }
    let logical = Rect::new(Au::ZERO, Au::ZERO, inline, block);
    let mut statics = crate::positioned::FixedStatics::default();
    layouter.prepare_page(&mut content, logical, (0, None), &mut statics);
    // Fixed boxes are placed in the viewport once, as absolutely positioned boxes are.
    layouter.position(&mut content, logical, Some((0, &statics)), None);
    if mode.is_vertical() {
        crate::writing::to_physical(&mut content, mode, (inline, block), &tree.flows);
    }
    let owners = layouter.stacking_owners.borrow();
    crate::stacking_owner::move_to_owners(&mut content, &owners);
    drop(owners);
    page.fragments.push(content);
    page
}

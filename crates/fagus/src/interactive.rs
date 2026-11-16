//! Links, link targets and bookmarks, found in the laid-out pages.

use fagus_dom::{Document, ElementData, NodeId};
use fagus_layout::{Fragment, FragmentKind, Page};
use fagus_pdf::{Bookmark, Link, LinkTarget};
use fagus_style::StyledDocument;
use fagus_style::values::{BookmarkLevel, ContentItem, ContentTarget, Display, Visibility};
use rustc_hash::FxHashMap;

/// The links of each page and the entries of the document outline.
pub(crate) fn collect(
    document: &Document,
    styled: &StyledDocument,
    pages: &[Page],
) -> (Vec<Vec<Link>>, Vec<Bookmark>) {
    let mut found = Found {
        styled: Some(styled),
        ..Found::default()
    };
    for (index, page) in pages.iter().enumerate() {
        // The first fragment is the page content, the others are margin boxes: their links work
        // on every page, but only the content has link targets and bookmarks.
        for (position, fragment) in page.fragments.iter().enumerate() {
            // Content laid out wider than the page shrinks from its corner, as it is painted.
            found.shrink = (position == 0 && page.scale > 0.0 && page.scale < 1.0)
                .then(|| (page.scale, fragment.rect.x.to_px(), fragment.rect.y.to_px()));
            // Right-to-left content is placed a fraction of a pixel further, inside the scale.
            found.shift = if position == 0 { page.shift } else { 0.0 };
            found.walk(document, fragment, index, position == 0, (0.0, 0.0));
        }
    }
    found.place_boxless_targets(document);
    let mut links: Vec<Vec<Link>> = vec![Vec::new(); pages.len()];
    for (page, rect, href, node) in found.links {
        if let Some(target) = target(&href, &found.targets) {
            links[page].push(Link {
                rect,
                target,
                node: Some(node),
                description: Some(description(document, node, &href)),
            });
        }
    }
    (links, found.bookmarks)
}

/// What a link is to a reader who does not see it: its label, its text, the text of its images,
/// or where it leads.
fn description(document: &Document, node: NodeId, href: &str) -> String {
    let element = document.element(node);
    let label = element
        .and_then(|element| {
            element
                .attribute("aria-label")
                .or_else(|| element.attribute("title"))
        })
        .map(normalize)
        .filter(|label| !label.is_empty());
    label
        .or_else(|| Some(normalize(&document.text_content(node))).filter(|text| !text.is_empty()))
        .or_else(|| {
            let alt: Vec<String> = document
                .descendants(node)
                .filter_map(|child| document.element(child))
                .filter(|child| child.is("img"))
                .filter_map(|image| image.attribute("alt").map(normalize))
                .filter(|alt| !alt.is_empty())
                .collect();
            Some(alt.join(" ")).filter(|alt| !alt.is_empty())
        })
        .unwrap_or_else(|| href.to_owned())
}

/// Text with white space collapsed.
fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Default)]
struct Found<'a> {
    /// The styles of the document, which decide what text of a heading is shown.
    styled: Option<&'a StyledDocument>,
    /// Page, area, `href` and element of each link fragment.
    links: Vec<(usize, [f32; 4], String, NodeId)>,
    /// Where each element with an `id` (or an anchor with a `name`) starts.
    targets: FxHashMap<String, (usize, f32, f32)>,
    /// Where the first box of each element in the page content starts.
    starts: FxHashMap<NodeId, (usize, f32, f32)>,
    bookmarks: Vec<Bookmark>,
    /// How the fragments being walked are shrunk on the page: a scale around a point.
    shrink: Option<(f32, f32, f32)>,
    /// How far right the fragments being walked are moved before they shrink.
    shift: f32,
    /// The `href` and element of the inline link around the fragments being walked: images and
    /// other atomic inlines inside it can be clicked where they reach past its lines, as in
    /// Chrome.
    inline_link: Option<(String, NodeId)>,
    /// Whether the fragments being walked are in a box of an opacity that Chrome does not draw
    /// at all: their links are not drawn either.
    unseen: bool,
}

impl Found<'_> {
    /// Gives the elements with an `id` (or the anchors with a `name`) that have no box of their
    /// own the place where the content after their start begins: an empty `<a name>` between
    /// two blocks, or one around a heading (as `LibreOffice` writes them), lies on an empty line
    /// that Chrome keeps, where Fagus keeps no line, just before that content. So do elements
    /// that are not rendered (`display: none` or `contents`, or inside an element of
    /// `display: none`): Chrome keeps the links to them, to a named destination it never
    /// defines, which leads nowhere; a link of a PDF leads somewhere, here to the content where
    /// the element would be (at the end of the document, to the last element with a box).
    fn place_boxless_targets(&mut self, document: &Document) {
        let Some(root) = document.document_element() else {
            return;
        };
        let mut waiting: Vec<String> = Vec::new();
        // Where the last element with a box before starts, for those at the end of the
        // document, after all content.
        let mut last = None;
        for node in document.descendants(root) {
            if let Some(element) = document.element(node) {
                let anchor = element.attribute("name").filter(|_| element.is("a"));
                for name in element.id.as_deref().into_iter().chain(anchor) {
                    if !self.targets.contains_key(name) {
                        waiting.push(name.to_owned());
                    }
                }
            }
            if let Some(&start) = self.starts.get(&node) {
                for name in waiting.drain(..) {
                    self.targets.entry(name).or_insert(start);
                }
                last = Some(start);
            }
        }
        if let Some(start) = last {
            for name in waiting {
                self.targets.entry(name).or_insert(start);
            }
        }
    }

    /// Where a point of the fragments being walked lies on the page.
    fn on_page(&self, x: f32, y: f32) -> (f32, f32) {
        let x = x + self.shift;
        match self.shrink {
            Some((scale, left, top)) => (left + (x - left) * scale, top + (y - top) * scale),
            None => (x, y),
        }
    }

    /// The area on the page of a link's part at `(x, y)` of `size`: shrunk with the content,
    /// and with its edges rounded to whole pixels, as Chrome writes them where the content is
    /// not shrunk.
    fn link_rect(&self, (x, y): (f32, f32), (width, height): (f32, f32)) -> [f32; 4] {
        let scale = self.shrink.map_or(1.0, |(scale, ..)| scale);
        let (left, top) = self.on_page(x, y);
        let (right, bottom) = (left + width * scale, top + height * scale);
        if self.shrink.is_some() {
            return [left, top, right - left, bottom - top];
        }
        let [left, top, right, bottom] = [left, top, right, bottom].map(f32::round);
        [left, top, right - left, bottom - top]
    }

    fn walk(
        &mut self,
        document: &Document,
        fragment: &Fragment,
        page: usize,
        content: bool,
        origin: (f32, f32),
    ) {
        let x = origin.0 + fragment.rect.x.to_px();
        let y = origin.1 + fragment.rect.y.to_px();
        let outer_link = self.inline_link.clone();
        let outer_unseen = self.unseen;
        if let FragmentKind::Box {
            style,
            node: Some(node),
            sides,
        } = &fragment.kind
            && let Some(element) = document.element(*node)
        {
            self.unseen |= fagus_paint::invisible_opacity(style.opacity);
            let atomic = style.display != Display::INLINE
                || fragment
                    .children
                    .iter()
                    .any(|child| matches!(child.kind, FragmentKind::Image(_)));
            let link = element
                .attribute("href")
                .filter(|_| element.is("a"))
                .map(|href| (href.trim().to_owned(), *node))
                .or_else(|| self.inline_link.clone().filter(|_| atomic));
            if let Some((href, node)) = link {
                // An empty or invisible box, such as an anchor without content, cannot be
                // clicked.
                if style.visibility == Visibility::Visible && !self.unseen {
                    for (start, end) in link_spans(fragment, atomic) {
                        let rect =
                            self.link_rect((x + start, y), (end - start, fragment.rect.height.to_px()));
                        if rect[2] > 0.0 && rect[3] > 0.0 {
                            self.links.push((page, rect, href.clone(), node));
                        }
                    }
                }
                // Inside an inline link, the atomic inlines are areas of their own; anything
                // inside a block or an atomic inline is covered by its area.
                self.inline_link = (!atomic).then_some((href, node));
            } else if atomic {
                self.inline_link = None;
            }
            // A box continued from an earlier page or line starts there.
            if content && sides.top && sides.left {
                let anchor = element.attribute("name").filter(|_| element.is("a"));
                let (left, top) = self.on_page(x, y);
                self.starts.entry(*node).or_insert((page, left, top));
                for name in element.id.as_deref().into_iter().chain(anchor) {
                    self.targets.entry(name.to_owned()).or_insert((page, left, top));
                }
                // `bookmark-level: none` leaves out a heading by role too.
                let level = match style.bookmark_level {
                    BookmarkLevel::Auto => aria_heading(element),
                    BookmarkLevel::None => None,
                    BookmarkLevel::Level(level) => Some(level),
                };
                if let Some(level) = level {
                    let title = label(document, self.styled, *node, style.bookmark_label.as_deref());
                    if !title.is_empty() {
                        self.bookmarks.push(Bookmark {
                            level,
                            title,
                            page,
                            x: left,
                            y: top,
                            open: !style.bookmark_closed,
                        });
                    }
                }
            }
        }
        for child in &fragment.children {
            self.walk(document, child, page, content, (x, y));
        }
        self.inline_link = outer_link;
        self.unseen = outer_unseen;
    }
}

/// The horizontal spans, from the left of `fragment`, of the areas that a link's box fragment
/// makes, as Chrome makes them: one for each inline box in it (`<a>one <b>two</b> three</a>`
/// makes three), and one for each stretch of text and atomic inlines between them, which
/// together cover the fragment. An atomic box (`inline-block`, a block) is one area.
fn link_spans(fragment: &Fragment, atomic: bool) -> Vec<(f32, f32)> {
    let width = fragment.rect.width.to_px();
    if atomic {
        return vec![(0.0, width)];
    }
    let boxes: Vec<(f32, f32)> = fragment
        .children
        .iter()
        .filter(|child| {
            matches!(&child.kind, FragmentKind::Box { style, .. }
                if style.display == Display::INLINE && !style.is_out_of_flow() && !style.is_floating()
                    && !child.children.iter().any(|inner| matches!(inner.kind, FragmentKind::Image(_))))
        })
        .map(|child| {
            (
                child.rect.x.to_px(),
                child.rect.x.to_px() + child.rect.width.to_px(),
            )
        })
        .collect();
    let has_other = |from: f32, to: f32| {
        fragment.children.iter().any(|child| {
            let left = child.rect.x.to_px();
            let right = left + child.rect.width.to_px();
            !matches!(&child.kind, FragmentKind::Box { style, .. } if style.display == Display::INLINE)
                && !matches!(child.kind, FragmentKind::Placeholder(_))
                && right > from + 0.01
                && left < to - 0.01
        })
    };
    let mut spans = Vec::new();
    let mut from = 0.0;
    for (index, &(left, right)) in boxes.iter().enumerate() {
        // The stretch before the box, where something is there (or it starts the fragment).
        if left > from + 0.01 && has_other(from, left) {
            spans.push((from, left));
        } else if let Some(last) = spans.last_mut().filter(|_| index > 0 && left > from + 0.01) {
            last.1 = left;
        }
        let start = if spans.is_empty() { 0.0 } else { left };
        spans.push((start, right));
        from = right;
    }
    match spans.last_mut() {
        None => spans.push((0.0, width)),
        Some(last) if !has_other(from, width) => last.1 = width,
        Some(_) => spans.push((from, width)),
    }
    spans
}

/// The level of an element with the ARIA role `heading` (`aria-level`, 2 without one), which
/// Chrome puts into the outline beside `<h1>` to `<h6>`.
pub(crate) fn aria_heading(element: &ElementData) -> Option<u8> {
    let role = element.attribute("role")?;
    if !role
        .split_ascii_whitespace()
        .next()?
        .eq_ignore_ascii_case("heading")
    {
        return None;
    }
    let level = element
        .attribute("aria-level")
        .and_then(|level| level.trim().parse::<u8>().ok())
        .filter(|level| *level >= 1)
        .unwrap_or(2);
    Some(level.min(6))
}

/// Where `href` leads: a target in the document (`#id`) or a URI with a scheme that is safe in a
/// PDF. Relative URLs and other schemes (`javascript:`, `file:`) lead nowhere.
fn target(href: &str, targets: &FxHashMap<String, (usize, f32, f32)>) -> Option<LinkTarget> {
    if let Some(name) = href.strip_prefix('#') {
        // The element with the name as it is or percent-decoded; without one, `#` and `#top`
        // lead to the top of the document (HTML, "find a potential indicated element").
        let found = targets.get(name).or_else(|| targets.get(&percent_decoded(name)?));
        return match found {
            Some(&(page, x, y)) => Some(LinkTarget::Position { page, x, y }),
            None if name.is_empty() || name.eq_ignore_ascii_case("top") => Some(LinkTarget::Position {
                page: 0,
                x: 0.0,
                y: 0.0,
            }),
            None => None,
        };
    }
    let (scheme, rest) = href.split_once(':')?;
    let scheme = scheme.to_ascii_lowercase();
    if !matches!(scheme.as_str(), "http" | "https" | "mailto" | "tel" | "ftp") {
        return None;
    }
    // Tabs and line breaks inside a URL are left out, and a web address without a path gets
    // the root, as in the URLs Chrome writes (`https://quittio.dev/`).
    let mut rest: String = rest
        .chars()
        .filter(|character| !matches!(character, '\t' | '\n' | '\r'))
        .collect();
    if scheme != "mailto"
        && scheme != "tel"
        && let Some(authority) = rest.strip_prefix("//")
    {
        let end = authority.find(['/', '?', '#']).map_or(rest.len(), |end| end + 2);
        if !rest[end..].starts_with('/') {
            rest.insert(end, '/');
        }
    }
    Some(LinkTarget::Uri(format!("{scheme}:{rest}")))
}

/// Text with its percent-encoded bytes (`%C3%BC`) decoded, if it has any and they are UTF-8.
fn percent_decoded(text: &str) -> Option<String> {
    if !text.contains('%') {
        return None;
    }
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        let hex = bytes
            .get(index + 1..index + 3)
            .and_then(|digits| std::str::from_utf8(digits).ok())
            .and_then(|digits| u8::from_str_radix(digits, 16).ok());
        match hex {
            Some(value) if byte == b'%' => {
                decoded.push(value);
                index += 3;
            }
            _ => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded).ok()
}

/// The title of a bookmark: `bookmark-label`, or the element's text.
fn label(
    document: &Document,
    styled: Option<&StyledDocument>,
    node: NodeId,
    items: Option<&[ContentItem]>,
) -> String {
    let text = || match styled {
        Some(styled) => shown_text(document, styled, node),
        None => normalize(&document.text_content(node)),
    };
    let Some(items) = items else {
        return text();
    };
    let label: String = items
        .iter()
        .map(|item| match item {
            ContentItem::String(value) => value.clone(),
            ContentItem::Attr(name) => document
                .element(node)
                .and_then(|element| element.attribute(name))
                .unwrap_or_default()
                .to_owned(),
            ContentItem::Content(ContentTarget::Text) => text(),
            _ => String::new(),
        })
        .collect();
    label.trim().to_owned()
}

/// The text of an element as it is shown, which Chrome takes for the title of its bookmark: the
/// text of what is drawn (not of boxes that are not rendered, hidden or of an opacity of zero),
/// transformed by `text-transform`, its white space collapsed (but for no-break spaces) and the
/// spaces before a line break and at its ends left out. Generated content is not part of it.
fn shown_text(document: &Document, styled: &StyledDocument, node: NodeId) -> String {
    fn collect(document: &Document, styled: &StyledDocument, node: NodeId, text: &mut String) {
        for child in document.children(node) {
            if let Some(element) = document.element(child) {
                let Some(style) = styled.style(child) else {
                    continue;
                };
                if style.display == Display::None || fagus_paint::invisible_opacity(style.opacity) {
                    continue;
                }
                if element.is("br") {
                    text.truncate(text.trim_end_matches(is_collapsible).len());
                    continue;
                }
                collect(document, styled, child, text);
            } else if let Some(data) = document.text(child)
                && let Some(style) = styled.style(node)
                && style.visibility == Visibility::Visible
            {
                let mut collapsed = String::with_capacity(data.len());
                let mut space = text.is_empty() || text.ends_with(is_collapsible);
                for character in data.chars() {
                    if is_collapsible(character) {
                        if !space {
                            collapsed.push(' ');
                        }
                        space = true;
                    } else {
                        collapsed.push(character);
                        space = false;
                    }
                }
                let shown = fagus_layout::transformed_text(&collapsed, style, text);
                text.push_str(&shown);
            }
        }
    }
    let mut text = String::new();
    collect(document, styled, node, &mut text);
    text.trim_matches(is_collapsible).to_owned()
}

/// Whether a character is white space that collapses (not a no-break space).
fn is_collapsible(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n' | '\r' | '\u{c}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrinks_links_and_targets_with_the_content() {
        use fagus_layout::units::{Au, Rect};
        use fagus_layout::{PageMarks, PageOrientation, VisibleSides};
        let document = Document::parse_html("<!DOCTYPE html><a id=top href='#top'>x</a>");
        let link = document.find("a").expect("link");
        let user_agent = fagus_css::Stylesheet::parse(
            fagus_style::USER_AGENT_CSS,
            fagus_css::Origin::UserAgent,
            "ua.css",
        );
        let rules = fagus_css::RuleSet::new(&[&user_agent], &fagus_css::MediaEnvironment::default());
        let styled = fagus_style::compute_styles(&document, &rules, (300.0, 300.0));
        let style = std::sync::Arc::new(fagus_style::ComputedStyle::default());
        // The content's corner at (10, 20), the link 100 by 20 at (40, 60) inside, shrunk by half.
        let page = Page {
            width: Au::from_px(300.0),
            height: Au::from_px(300.0),
            fragments: vec![Fragment {
                rect: Rect::new(
                    Au::from_px(10.0),
                    Au::from_px(20.0),
                    Au::from_px(400.0),
                    Au::from_px(200.0),
                ),
                kind: FragmentKind::Anonymous,
                children: vec![Fragment {
                    rect: Rect::new(
                        Au::from_px(30.0),
                        Au::from_px(40.0),
                        Au::from_px(100.0),
                        Au::from_px(20.0),
                    ),
                    kind: FragmentKind::Box {
                        style: std::sync::Arc::clone(&style),
                        node: Some(link),
                        sides: VisibleSides::ALL,
                    },
                    children: Vec::new().into(),
                }]
                .into(),
            }],
            margin: [Au::ZERO; 4],
            style,
            canvas: None,
            bleed: 0.0,
            marks: PageMarks::default(),
            orientation: PageOrientation::Upright,
            scale: 0.5,
            shift: 0.0,
            carried: Vec::new(),
            area: Rect::new(
                Au::from_px(10.0),
                Au::from_px(20.0),
                Au::from_px(200.0),
                Au::from_px(100.0),
            ),
        };
        let (links, _) = collect(&document, &styled, std::slice::from_ref(&page));
        assert_eq!(links[0][0].rect, [25.0, 40.0, 50.0, 10.0]);
        assert_eq!(
            links[0][0].target,
            LinkTarget::Position {
                page: 0,
                x: 25.0,
                y: 40.0
            }
        );
        // Right to left, the content moves right inside the scale, as it is painted.
        let shifted = Page { shift: 0.5, ..page };
        let (links, _) = collect(&document, &styled, &[shifted]);
        assert_eq!(links[0][0].rect, [25.25, 40.0, 50.0, 10.0]);
    }

    #[test]
    fn titles_bookmarks_with_the_text_shown() {
        // Sphinx's `¶` is hidden, Docusaurus' anchor has no opacity, Redoc's headings are
        // uppercase and a line break takes the space before it, as in Chrome's outline.
        let document = Document::parse_html(
            "<!DOCTYPE html><style>.h { visibility: hidden } .o { opacity: 0 } .n { display: none }</style>\
             <h2 id=a>API <a class=h>¶</a></h2>\
             <h2 id=b>Webhooks<a class=o>&#8203;</a><span class=n>x</span></h2>\
             <h5 id=c style='text-transform: uppercase'>Request body</h5>\
             <h2 id=d>Many   spaces <br>and&nbsp;break&nbsp;</h2>",
        );
        let user_agent = fagus_css::Stylesheet::parse(
            fagus_style::USER_AGENT_CSS,
            fagus_css::Origin::UserAgent,
            "ua.css",
        );
        let author = fagus_css::Stylesheet::parse(
            ".h { visibility: hidden } .o { opacity: 0 } .n { display: none }",
            fagus_css::Origin::Author,
            "doc.css",
        );
        let rules = fagus_css::RuleSet::new(&[&user_agent, &author], &fagus_css::MediaEnvironment::default());
        let styled = fagus_style::compute_styles(&document, &rules, (300.0, 300.0));
        let title = |id: &str| {
            let node = document
                .descendants(document.document_element().expect("root"))
                .find(|&node| {
                    document
                        .element(node)
                        .is_some_and(|element| element.id.as_deref() == Some(id))
                })
                .expect("heading");
            label(&document, Some(&styled), node, None)
        };
        assert_eq!(title("a"), "API");
        assert_eq!(title("b"), "Webhooks");
        assert_eq!(title("c"), "REQUEST BODY");
        assert_eq!(title("d"), "Many spacesand\u{a0}break\u{a0}");
    }

    #[test]
    fn keeps_safe_link_targets() {
        let mut targets = FxHashMap::default();
        targets.insert("summen".to_owned(), (1, 10.0, 20.0));
        assert_eq!(
            target("#summen", &targets),
            Some(LinkTarget::Position {
                page: 1,
                x: 10.0,
                y: 20.0
            })
        );
        assert_eq!(target("#fehlt", &targets), None);
        assert_eq!(
            target("https://quittio.dev", &targets),
            Some(LinkTarget::Uri("https://quittio.dev/".to_owned()))
        );
        assert_eq!(
            target("mailto:rechnung@example.com", &targets),
            Some(LinkTarget::Uri("mailto:rechnung@example.com".to_owned()))
        );
        assert_eq!(
            target("HTTPS://quittio.dev?a=1#b", &targets),
            Some(LinkTarget::Uri("https://quittio.dev/?a=1#b".to_owned()))
        );
        assert_eq!(
            target("https://quittio.dev/a\n/b", &targets),
            Some(LinkTarget::Uri("https://quittio.dev/a/b".to_owned()))
        );
        assert_eq!(target("javascript:alert(1)", &targets), None);
        assert_eq!(target("file:///etc/passwd", &targets), None);
        assert_eq!(target("agb.html", &targets), None);
    }

    #[test]
    fn leads_empty_fragments_and_top_to_the_top_as_html() {
        let mut targets = FxHashMap::default();
        // `#` and `#top` lead to the top unless an element has the name; names may be encoded.
        let top = Some(LinkTarget::Position {
            page: 0,
            x: 0.0,
            y: 0.0,
        });
        assert_eq!(target("#", &targets), top);
        assert_eq!(target("#Top", &targets), top);
        targets.insert("top".to_owned(), (2, 0.0, 30.0));
        targets.insert("über".to_owned(), (3, 0.0, 40.0));
        assert_eq!(
            target("#top", &targets),
            Some(LinkTarget::Position {
                page: 2,
                x: 0.0,
                y: 30.0
            })
        );
        assert_eq!(
            target("#%C3%BCber", &targets),
            Some(LinkTarget::Position {
                page: 3,
                x: 0.0,
                y: 40.0
            })
        );
        assert_eq!(target("#%zz", &targets), None);
    }
}

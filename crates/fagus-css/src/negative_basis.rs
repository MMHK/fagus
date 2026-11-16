//! Percentages compared in `min()`, `max()` and `clamp()` where the size they are percentages of
//! may be negative.
//!
//! A percentage of a background position is one of the area less the image (CSS Backgrounds 3,
//! §3.6), which is negative where the image is the larger: `min(0%, 100%)` is then `100%`.
//! lightningcss compares the percentages when it parses them and keeps `0%`. Chrome keeps them
//! in `background-position` (but as the offset from an edge keyword), and elsewhere (`background`, `background-position-x`,
//! `object-position`) wherever the comparison is not the whole value or has arguments of other
//! types (`calc(min(0%, 100%) + 0px)`, `min(0%, 100%, 10px)`); a bare `min(0%, 100%)` there it
//! reduces to `0%` too. Where Chrome keeps them, each percentage is written as a sum with a zero
//! that lightningcss cannot reduce (`calc(0% + min(0px, 0em))`), so that the comparison waits
//! for the size.

use cssparser::{Parser, ParserInput, Token};

/// The properties whose percentages may be ones of a negative size.
const POSITIONS: &[&str] = &[
    "background",
    "background-position",
    "background-position-x",
    "background-position-y",
    "object-position",
];

/// What a percentage is written out as around it, so that lightningcss keeps it apart.
const BEFORE: &str = "calc(";
const AFTER: &str = " + min(0px, 0em))";

/// A significant token of a function's arguments.
enum Node {
    /// A percentage, where its text is.
    Percentage(usize, usize),
    /// A function or parenthesized block (with no name), with its arguments.
    Function(String, Vec<Vec<Node>>),
    Other,
}

/// Whether the percentages of `property`'s value may be of a negative size.
pub(crate) fn applies(property: &str) -> bool {
    POSITIONS.contains(&property)
}

/// Notes the edits that keep the percentages compared in the function `css[start..end]` (a
/// whole function at the top level of the value of `property`; `offset` where it follows a
/// keyword, as the offset from an edge in `right min(0%, 100%)`) from being reduced where Chrome
/// keeps them.
pub(crate) fn protect(
    property: &str,
    css: &str,
    (start, end): (usize, usize),
    offset: bool,
    edits: &mut Vec<(usize, &'static str)>,
) {
    let Some(text) = css.get(start..end) else {
        return;
    };
    let mut input = ParserInput::new(text);
    let mut parser = Parser::new(&mut input);
    let Ok(Token::Function(name)) = parser.next() else {
        return;
    };
    let name = name.to_ascii_lowercase();
    let args = arguments(&mut parser);
    let node = Node::Function(name, args);
    visit(
        &node,
        true,
        property == "background-position" && !offset,
        start,
        edits,
    );
}

/// The arguments of the block the parser is at the start of.
fn arguments(parser: &mut Parser<'_, '_>) -> Vec<Vec<Node>> {
    parser
        .parse_nested_block(|block| {
            let mut args = vec![Vec::new()];
            loop {
                let start = block.position().byte_index();
                let node = match block.next_including_whitespace_and_comments() {
                    Err(_) => break,
                    Ok(Token::WhiteSpace(_) | Token::Comment(_)) => continue,
                    Ok(Token::Comma) => {
                        args.push(Vec::new());
                        continue;
                    }
                    Ok(Token::Percentage { .. }) => Node::Percentage(start, block.position().byte_index()),
                    Ok(Token::Function(name)) => {
                        let name = name.to_ascii_lowercase();
                        Node::Function(name, arguments(block))
                    }
                    Ok(Token::ParenthesisBlock) => Node::Function(String::new(), arguments(block)),
                    Ok(_) => Node::Other,
                };
                if let Some(arg) = args.last_mut() {
                    arg.push(node);
                }
            }
            Ok::<_, cssparser::ParseError<'_, ()>>(args)
        })
        .unwrap_or_default()
}

/// Notes the edits for the comparisons in `node`; `root` says whether it is the whole of its
/// math expression (inside `calc()` or parentheses with nothing else), and `keep` whether Chrome
/// keeps even such a comparison.
fn visit(node: &Node, root: bool, keep: bool, offset: usize, edits: &mut Vec<(usize, &'static str)>) {
    let Node::Function(name, args) = node else {
        return;
    };
    if matches!(
        name.as_str(),
        "url" | "var" | "env" | "attr" | "image-set" | "-webkit-image-set"
    ) {
        return;
    }
    if matches!(name.as_str(), "min" | "max" | "clamp") {
        let percentages: Vec<(usize, usize)> = args
            .iter()
            .filter_map(|arg| match arg.as_slice() {
                [Node::Percentage(start, end)] => Some((*start, *end)),
                _ => None,
            })
            .collect();
        let reduced = root && !keep && percentages.len() == args.len();
        if percentages.len() >= 2 && !reduced {
            for (start, end) in percentages {
                edits.push((offset + start, BEFORE));
                edits.push((offset + end, AFTER));
            }
        }
    }
    // A function alone in `calc()` or parentheses is still the whole expression.
    let wraps = matches!(name.as_str(), "calc" | "") && args.len() == 1 && args[0].len() == 1;
    for arg in args {
        for child in arg {
            visit(child, root && wraps, keep, offset, edits);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::author_css;

    #[test]
    fn keeps_percentages_compared_where_chrome_does() {
        let standard = |css: &str| author_css(css, false);
        assert_eq!(
            standard("div { background-position: min(0%, 100%) MAX(0%, 100%) }").as_deref(),
            Some(
                "div { background-position: min(calc(0% + min(0px, 0em)), calc(100% + min(0px, 0em))) \
                 MAX(calc(0% + min(0px, 0em)), calc(100% + min(0px, 0em))) }"
            )
        );
        // Elsewhere only inside larger expressions, or beside values of other types.
        assert_eq!(
            standard("div { background-position: right min(0%, 100%) top 0 }"),
            None
        );
        assert_eq!(
            standard("div { background-position-x: min(0%, 100%); object-position: calc(max(0%, 50%)) }"),
            None
        );
        assert_eq!(
            standard("div { background: url(a.png) calc(min(0%, 100%) + 0px) 0 }").as_deref(),
            Some(
                "div { background: url(a.png) calc(min(calc(0% + min(0px, 0em)), calc(100% + min(0px, 0em))) + 0px) 0 }"
            )
        );
        assert_eq!(
            standard("div { background-position-x: clamp(0%, 10px, 100%) }").as_deref(),
            Some(
                "div { background-position-x: clamp(calc(0% + min(0px, 0em)), 10px, calc(100% + min(0px, 0em))) }"
            )
        );
        // Not in other properties, nor with one percentage.
        assert_eq!(
            standard("div { width: calc(min(0%, 100%) + 0px); background-position: min(0%, 1px) }"),
            None
        );
    }

    #[test]
    fn compares_protected_percentages_against_the_size() {
        use lightningcss::properties::Property;
        use lightningcss::values::length::LengthPercentage;
        use lightningcss::values::position::PositionComponent;
        let css = author_css("background-position-x: calc(min(0%, 100%) + 0px)", false).unwrap_or_default();
        let block = crate::parse_declarations(&css).expect("declarations");
        let Some(Property::BackgroundPositionX(positions)) = block.declarations.declarations.first() else {
            panic!("no background-position-x in {css}");
        };
        // The comparison stays: it is no single percentage.
        assert!(matches!(
            positions.first(),
            Some(PositionComponent::Length(LengthPercentage::Calc(_)))
        ));
    }
}

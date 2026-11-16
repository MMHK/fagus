//! The `flow-start` and `flow-end` positions of `align-content` and `justify-content`, which
//! Chrome 157 takes (as `start` and `end` outside grid lanes) and lightningcss does not parse.

use lightningcss::properties::PropertyId;
use lightningcss::properties::custom::{Token, TokenOrValue, UnparsedProperty};

/// `align-content` or `justify-content` of `flow-start` or `flow-end`, with `safe` or `unsafe`
/// before it or not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlowPosition {
    /// `align-content` rather than `justify-content`.
    pub align: bool,
    /// `flow-end` rather than `flow-start`.
    pub end: bool,
    /// `safe` (`Some(true)`), `unsafe` (`Some(false)`) or neither.
    pub safe: Option<bool>,
}

/// The flow position an unparsed declaration gives, if it is one.
pub fn flow_position(unparsed: &UnparsedProperty<'_>) -> Option<FlowPosition> {
    let align = match unparsed.property_id {
        PropertyId::AlignContent(_) => true,
        PropertyId::JustifyContent(_) => false,
        _ => return None,
    };
    let mut idents = unparsed.value.0.iter().filter_map(|token| match token {
        TokenOrValue::Token(Token::WhiteSpace(_)) => None,
        TokenOrValue::Token(Token::Ident(ident)) => Some(Some(ident.to_ascii_lowercase())),
        _ => Some(None),
    });
    let mut first = idents.next().flatten();
    let safe = match first.as_deref() {
        Some("safe") => Some(true),
        Some("unsafe") => Some(false),
        _ => None,
    };
    if safe.is_some() {
        first = idents.next().flatten();
    }
    let end = match first.as_deref() {
        Some("flow-start") => false,
        Some("flow-end") => true,
        _ => return None,
    };
    idents
        .next()
        .is_none()
        .then_some(FlowPosition { align, end, safe })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightningcss::properties::Property;
    use lightningcss::stylesheet::ParserOptions;

    fn parse(name: &str, value: &str) -> Option<FlowPosition> {
        let property =
            Property::parse_string(PropertyId::from(name), value, ParserOptions::default()).ok()?;
        match property {
            Property::Unparsed(unparsed) => flow_position(&unparsed),
            _ => None,
        }
    }

    #[test]
    fn reads_flow_positions() {
        assert_eq!(
            parse("align-content", "flow-end"),
            Some(FlowPosition {
                align: true,
                end: true,
                safe: None
            })
        );
        assert_eq!(
            parse("justify-content", "unsafe flow-start"),
            Some(FlowPosition {
                align: false,
                end: false,
                safe: Some(false)
            })
        );
        assert_eq!(parse("align-content", "flow-end flow-start"), None);
        assert_eq!(parse("align-items", "flow-end"), None);
    }
}

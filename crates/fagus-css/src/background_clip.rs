//! `background-clip` values of CSS Backgrounds 4 that lightningcss does not parse:
//! `border-area`, alone or with `text`.

use lightningcss::properties::custom::{Token, TokenList, TokenOrValue};

/// The box a background layer is clipped to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipBox {
    Border,
    Padding,
    Content,
}

/// One layer of `background-clip` (`<visual-box> | [ border-area || text ]`): its box, and
/// whether it shows only through the box's text and where its borders paint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClipLayer {
    pub area: ClipBox,
    pub text: bool,
    pub border_area: bool,
}

/// The layers of a `background-clip` value, comma-separated; `None` where one is invalid.
pub fn layers(tokens: &TokenList<'_>) -> Option<Vec<ClipLayer>> {
    tokens
        .0
        .split(|token| matches!(token, TokenOrValue::Token(Token::Comma)))
        .map(|layer| {
            let mut words = Vec::new();
            for token in layer {
                match token {
                    TokenOrValue::Token(Token::Ident(ident)) => words.push(ident.to_ascii_lowercase()),
                    TokenOrValue::Token(Token::WhiteSpace(_)) => {}
                    _ => return None,
                }
            }
            let plain = |area| {
                Some(ClipLayer {
                    area,
                    text: false,
                    border_area: false,
                })
            };
            match words.iter().map(String::as_str).collect::<Vec<_>>()[..] {
                ["border-box" | "border"] => plain(ClipBox::Border),
                ["padding-box" | "padding"] => plain(ClipBox::Padding),
                ["content-box" | "content"] => plain(ClipBox::Content),
                [first, ref rest @ ..] if rest.len() <= 1 => {
                    let mut layer = ClipLayer {
                        area: ClipBox::Border,
                        text: false,
                        border_area: false,
                    };
                    for word in std::iter::once(first).chain(rest.iter().copied()) {
                        let flag = match word {
                            "text" => &mut layer.text,
                            "border-area" => &mut layer.border_area,
                            _ => return None,
                        };
                        if std::mem::replace(flag, true) {
                            return None;
                        }
                    }
                    Some(layer)
                }
                _ => None,
            }
        })
        .collect()
}

/// Whether a `background-clip` that lightningcss keeps unparsed is a valid one with
/// `border-area`, which Fagus parses with [`layers`].
pub fn is_border_area(unparsed: &lightningcss::properties::custom::UnparsedProperty<'_>) -> bool {
    matches!(
        unparsed.property_id,
        lightningcss::properties::PropertyId::BackgroundClip(_)
    ) && layers(&unparsed.value).is_some_and(|layers| layers.iter().any(|layer| layer.border_area))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightningcss::properties::{Property, PropertyId};
    use lightningcss::stylesheet::ParserOptions;

    fn parse(value: &str) -> Option<Vec<ClipLayer>> {
        match Property::parse_string(
            PropertyId::from("background-clip"),
            value,
            ParserOptions::default(),
        ) {
            Ok(Property::Unparsed(unparsed)) => layers(&unparsed.value),
            _ => None,
        }
    }

    #[test]
    fn parses_border_area_alone_and_with_text() {
        let layer = |area, text, border_area| ClipLayer {
            area,
            text,
            border_area,
        };
        assert_eq!(
            parse("border-area"),
            Some(vec![layer(ClipBox::Border, false, true)])
        );
        assert_eq!(
            parse("text border-area, content-box"),
            Some(vec![
                layer(ClipBox::Border, true, true),
                layer(ClipBox::Content, false, false)
            ])
        );
        assert_eq!(parse("border-area border-area"), None);
        assert_eq!(parse("border-area padding-box"), None);
        assert_eq!(parse("border-area 2px"), None);
    }
}

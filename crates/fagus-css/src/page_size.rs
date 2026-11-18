//! The `size` descriptor of `@page` (CSS Paged Media 3, §7.1), with Chrome's grammar, and what
//! is wrong with a value that it does not take.

use lightningcss::properties::Property;
use lightningcss::properties::custom::{CustomProperty, CustomPropertyName, Token, TokenList, TokenOrValue};
use lightningcss::stylesheet::PrinterOptions;
use lightningcss::traits::Parse;
use lightningcss::values::calc::{Calc, MathFunction};
use lightningcss::values::ident::DashedIdent;
use lightningcss::values::length::{Length, LengthValue};

const MM: f32 = 96.0 / 25.4;
const INCH: f32 = 96.0;

/// The size of an A4 page, in pixels: the default page size.
pub const A4: (f32, f32) = (210.0 * MM, 297.0 * MM);

/// The names of page sizes that `size` takes, as Chrome knows them.
const NAMES: &str = "A5, A4, A3, B5, B4, JIS-B5, JIS-B4, letter, legal and ledger";

/// The size of a page named in the `size` descriptor of `@page`, portrait, in pixels: the names
/// Chrome knows (A6 and the other sizes of CSS Paged Media 3 it does not).
fn named_page_size(name: &str) -> Option<(f32, f32)> {
    Some(match name {
        "a5" => (148.0 * MM, 210.0 * MM),
        "a4" => A4,
        "a3" => (297.0 * MM, 420.0 * MM),
        "b5" => (176.0 * MM, 250.0 * MM),
        "b4" => (250.0 * MM, 353.0 * MM),
        "jis-b5" => (182.0 * MM, 257.0 * MM),
        "jis-b4" => (257.0 * MM, 364.0 * MM),
        "letter" => (8.5 * INCH, 11.0 * INCH),
        "legal" => (8.5 * INCH, 14.0 * INCH),
        "ledger" => (11.0 * INCH, 17.0 * INCH),
        _ => return None,
    })
}

/// The width and height in millimetres of an ISO 216 or ISO 269 paper size (A0 to A10, B0 to
/// B10, C0 to C10) or of a JIS B size (JIS-B0 to JIS-B10), portrait: each is the one before it
/// halved, rounded down to whole millimetres.
fn paper_size_mm(name: &str) -> Option<(u32, u32)> {
    let (largest, number) = if let Some(number) = name.strip_prefix("jis-b") {
        ((1030, 1456), number)
    } else {
        let mut characters = name.chars();
        let largest = match characters.next()? {
            'a' => (841, 1189),
            'b' => (1000, 1414),
            'c' => (917, 1297),
            _ => return None,
        };
        (largest, characters.as_str())
    };
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let number: u32 = number.parse().ok().filter(|number| *number <= 10)?;
    let (mut width, mut height) = largest;
    for _ in 0..number {
        (width, height) = (height / 2, width);
    }
    Some((width, height))
}

/// A length in the `size` descriptor of `@page`, in pixels: font-relative units are relative to
/// the initial font size of 16 pixels, viewport units to the default page, and `calc()`, `min()`,
/// `max()` and `clamp()` of such lengths count, as in Chrome (percentages do not).
fn page_size_length(token: &TokenOrValue<'_>) -> Option<f32> {
    match token {
        TokenOrValue::Length(length) => page_length_value(length),
        TokenOrValue::Token(Token::Number { value, .. }) if *value == 0.0 => Some(0.0),
        TokenOrValue::Function(_) => {
            let css = Property::Custom(CustomProperty {
                name: CustomPropertyName::Custom(DashedIdent("--size".into())),
                value: TokenList(vec![token.clone()]),
            })
            .value_to_css_string(PrinterOptions::default())
            .ok()?;
            page_length(&Length::parse_string(css.trim()).ok()?)
        }
        _ => None,
    }
}

/// A length of the page context in pixels (see [`page_size_length`]).
fn page_length(length: &Length) -> Option<f32> {
    fn calc(node: &Calc<Length>) -> Option<f32> {
        let all = |nodes: &[Calc<Length>]| nodes.iter().map(calc).collect::<Option<Vec<f32>>>();
        match node {
            Calc::Value(length) => page_length(length),
            Calc::Sum(a, b) => Some(calc(a)? + calc(b)?),
            Calc::Product(factor, node) => Some(factor * calc(node)?),
            Calc::Function(function) => match &**function {
                MathFunction::Calc(node) => calc(node),
                MathFunction::Min(nodes) => all(nodes)?.into_iter().reduce(f32::min),
                MathFunction::Max(nodes) => all(nodes)?.into_iter().reduce(f32::max),
                MathFunction::Clamp(low, value, high) => Some(calc(value)?.min(calc(high)?).max(calc(low)?)),
                _ => None,
            },
            Calc::Number(_) => None,
        }
    }
    let px = match length {
        Length::Value(value) => page_length_value(value)?,
        Length::Calc(node) => calc(node)?,
    };
    px.is_finite().then_some(px)
}

/// A length value of the page context in pixels (see [`page_size_length`]).
fn page_length_value(value: &LengthValue) -> Option<f32> {
    let (width, height) = A4;
    Some(match value {
        LengthValue::Em(value) | LengthValue::Rem(value) => value * 16.0,
        LengthValue::Vw(value)
        | LengthValue::Svw(value)
        | LengthValue::Lvw(value)
        | LengthValue::Dvw(value) => value * width / 100.0,
        LengthValue::Vh(value)
        | LengthValue::Svh(value)
        | LengthValue::Lvh(value)
        | LengthValue::Dvh(value) => value * height / 100.0,
        LengthValue::Vmin(value)
        | LengthValue::Svmin(value)
        | LengthValue::Lvmin(value)
        | LengthValue::Dvmin(value) => value * width.min(height) / 100.0,
        LengthValue::Vmax(value)
        | LengthValue::Svmax(value)
        | LengthValue::Lvmax(value)
        | LengthValue::Dvmax(value) => value * width.max(height) / 100.0,
        other => other.to_px()?,
    })
}

/// The tokens of a value other than white space.
fn significant<'a, 'i>(tokens: &'a [TokenOrValue<'i>]) -> Vec<&'a TokenOrValue<'i>> {
    tokens
        .iter()
        .filter(|token| !matches!(token, TokenOrValue::Token(Token::WhiteSpace(_))))
        .collect()
}

/// Parses the `size` descriptor of `@page` into the page's width and height in pixels: `auto`
/// (the default size), one or two non-negative lengths, or a page size name and an orientation,
/// each at most once and in either order (CSS Paged Media 3, §7.1). Anything else is invalid
/// (`None`) and leaves the size as it is, as in Chrome.
pub fn page_size(tokens: &[TokenOrValue<'_>]) -> Option<(f32, f32)> {
    let tokens = significant(tokens);
    let idents: Option<Vec<String>> = tokens
        .iter()
        .map(|token| match token {
            TokenOrValue::Token(Token::Ident(ident)) => Some(ident.to_ascii_lowercase()),
            _ => None,
        })
        .collect();
    let Some(idents) = idents else {
        let lengths: Option<Vec<f32>> = tokens.iter().map(|token| page_size_length(token)).collect();
        return match lengths?.as_slice() {
            [side] if *side >= 0.0 => Some((*side, *side)),
            [width, height] if *width >= 0.0 && *height >= 0.0 => Some((*width, *height)),
            _ => None,
        };
    };
    let mut size = None;
    let mut landscape = None;
    for ident in &idents {
        match ident.as_str() {
            "auto" if idents.len() == 1 => return Some(A4),
            "landscape" | "portrait" if landscape.is_none() => landscape = Some(ident == "landscape"),
            name if size.is_none() => size = Some(named_page_size(name)?),
            _ => return None,
        }
    }
    let (width, height) = size.unwrap_or(A4);
    Some(match landscape {
        Some(true) => (width.max(height), width.min(height)),
        _ => (width, height),
    })
}

/// What is wrong with a value of `size` that [`page_size`] does not take, for the diagnostic
/// that reports it dropped: `None` where it is valid.
pub fn page_size_problem(tokens: &TokenList<'_>) -> Option<String> {
    if page_size(&tokens.0).is_some() {
        return None;
    }
    let tokens = significant(&tokens.0);
    let ident = |token: &TokenOrValue<'_>| match token {
        TokenOrValue::Token(Token::Ident(ident)) => Some(ident.to_string()),
        _ => None,
    };
    let is_auto =
        |token: &&TokenOrValue<'_>| ident(token).is_some_and(|name| name.eq_ignore_ascii_case("auto"));
    let lengths: Vec<Option<f32>> = tokens.iter().map(|token| page_size_length(token)).collect();
    let length_count = lengths.iter().flatten().count();
    let general = format!(
        "`size` takes `auto`, one length or two (width and height), or a page size name ({NAMES}) with `portrait` or `landscape`"
    );
    // `80mm auto`: a page does not grow with its content.
    if length_count > 0 && tokens.iter().any(is_auto) {
        let axis = if tokens.first().is_some_and(is_auto) {
            "width"
        } else {
            "height"
        };
        return Some(format!(
            "a page's {axis} cannot follow its content; give both lengths, width and height, as in `80mm 200mm`"
        ));
    }
    if length_count == tokens.len() {
        return Some(if lengths.iter().flatten().any(|length| *length < 0.0) {
            "a page cannot be smaller than nothing".to_owned()
        } else {
            "a page takes one length (a square) or two (width and height)".to_owned()
        });
    }
    if tokens
        .iter()
        .any(|token| matches!(token, TokenOrValue::Token(Token::Percentage { .. })))
    {
        return Some("percentages do not work in `size`; give lengths".to_owned());
    }
    // A name that Chrome does not know, maybe a paper size given in millimetres instead.
    let names: Vec<String> = tokens.iter().filter_map(|token| ident(token)).collect();
    if names.len() != tokens.len() {
        return Some(general);
    }
    let landscape = names.iter().any(|name| name.eq_ignore_ascii_case("landscape"));
    let unknown = names.iter().find(|name| {
        let lower = name.to_ascii_lowercase();
        !matches!(lower.as_str(), "auto" | "portrait" | "landscape") && named_page_size(&lower).is_none()
    });
    match unknown {
        Some(name) => Some(match paper_size_mm(&name.to_ascii_lowercase()) {
            Some((width, height)) => {
                let (width, height) = if landscape {
                    (height, width)
                } else {
                    (width, height)
                };
                format!("Chrome does not know the page size `{name}`; write `{width}mm {height}mm`")
            }
            None => format!("`{name}` is not a page size name; the names are {NAMES}"),
        }),
        None => Some(general),
    }
}

#[cfg(test)]
mod tests {
    use lightningcss::stylesheet::ParserOptions;

    use super::*;

    fn tokens(value: &str) -> TokenList<'static> {
        use lightningcss::traits::IntoOwned;
        let property =
            Property::parse_string("size".into(), value, ParserOptions::default()).expect("parses");
        match property {
            Property::Custom(custom) => custom.value.into_owned(),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_sizes_as_chrome() {
        let size = |value: &str| page_size(&tokens(value).0);
        assert_eq!(size("auto"), Some(A4));
        assert_eq!(size("A5 landscape"), Some((210.0 * MM, 148.0 * MM)));
        assert_eq!(size("landscape a5"), Some((210.0 * MM, 148.0 * MM)));
        assert_eq!(size("100px 200px"), Some((100.0, 200.0)));
        assert_eq!(size("1in"), Some((96.0, 96.0)));
        for invalid in [
            "A6",
            "80mm auto",
            "1px 2px 3px",
            "auto landscape",
            "A4 A4",
            "-1px",
            "50%",
        ] {
            assert_eq!(size(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn tells_what_is_wrong_with_a_size() {
        let problem = |value: &str| page_size_problem(&tokens(value));
        assert_eq!(problem("A4 landscape"), None);
        assert_eq!(problem("80mm 200mm"), None);
        assert_eq!(
            problem("80mm auto").as_deref(),
            Some(
                "a page's height cannot follow its content; give both lengths, width and height, as in `80mm 200mm`"
            )
        );
        assert!(problem("auto 200mm").is_some_and(|message| message.starts_with("a page's width")));
        assert_eq!(
            problem("A6").as_deref(),
            Some("Chrome does not know the page size `A6`; write `105mm 148mm`")
        );
        assert_eq!(
            problem("landscape a6").as_deref(),
            Some("Chrome does not know the page size `a6`; write `148mm 105mm`")
        );
        assert!(problem("C5").is_some_and(|message| message.ends_with("`162mm 229mm`")));
        assert!(problem("JIS-B6").is_some_and(|message| message.ends_with("`128mm 182mm`")));
        assert!(problem("B10").is_some_and(|message| message.ends_with("`31mm 44mm`")));
        assert_eq!(
            problem("tabloid").as_deref(),
            Some(
                "`tabloid` is not a page size name; the names are A5, A4, A3, B5, B4, JIS-B5, JIS-B4, letter, legal and ledger"
            )
        );
        assert_eq!(
            problem("1px 2px 3px").as_deref(),
            Some("a page takes one length (a square) or two (width and height)")
        );
        assert_eq!(
            problem("-1px").as_deref(),
            Some("a page cannot be smaller than nothing")
        );
        assert_eq!(
            problem("50% 50%").as_deref(),
            Some("percentages do not work in `size`; give lengths")
        );
        assert!(problem("A4 A4").is_some_and(|message| message.starts_with("`size` takes `auto`")));
        assert!(problem("A4 10mm").is_some_and(|message| message.starts_with("`size` takes `auto`")));
    }
}

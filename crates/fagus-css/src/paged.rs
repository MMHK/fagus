//! Values of CSS Generated Content for Paged Media that lightningcss does not parse.

use lightningcss::properties::custom::{Token, TokenList, TokenOrValue};

/// The name in `position: running(name)` (CSS GCPM 3, §2.1).
pub fn running_name(tokens: &TokenList<'_>) -> Option<String> {
    let [TokenOrValue::Function(function)] = significant(tokens).as_slice() else {
        return None;
    };
    if !function.name.0.eq_ignore_ascii_case("running") {
        return None;
    }
    match significant(&function.arguments).as_slice() {
        [TokenOrValue::Token(Token::Ident(name))] => Some(name.to_string()),
        _ => None,
    }
}

/// The line names of `subgrid` in `grid-template-columns` or `grid-template-rows`, which
/// lightningcss does not parse: one list per `[…]`, in order, with `repeat(n, …)` written out;
/// `None` if the value is not `subgrid`.
pub fn subgrid_line_names(tokens: &TokenList<'_>) -> Option<Vec<Vec<String>>> {
    subgrid_lines(tokens).map(|lines| lines.before)
}

/// The line names of a `subgrid` value (CSS Grid 2, §9.3): the lists before an automatic
/// repetition (`repeat(auto-fill, …)`), its lists, and those after it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SubgridLines {
    pub before: Vec<Vec<String>>,
    pub repeated: Option<Vec<Vec<String>>>,
    pub after: Vec<Vec<String>>,
}

/// The most lists a `repeat(n, …)` writes out.
const MAX_REPEATED_LISTS: usize = 10_000;

/// The line names of a `subgrid` value (see [`SubgridLines`]); `None` if the value is not
/// `subgrid` or not valid.
pub fn subgrid_lines(tokens: &TokenList<'_>) -> Option<SubgridLines> {
    let tokens = significant(tokens);
    let (first, rest) = tokens.split_first()?;
    match first {
        TokenOrValue::Token(Token::Ident(ident)) if ident.eq_ignore_ascii_case("subgrid") => {}
        _ => return None,
    }
    let mut lines = SubgridLines::default();
    let mut current: Option<Vec<String>> = None;
    for token in rest {
        let target = if lines.repeated.is_some() {
            &mut lines.after
        } else {
            &mut lines.before
        };
        match (token, &mut current) {
            (TokenOrValue::Token(Token::SquareBracketBlock), None) => current = Some(Vec::new()),
            (TokenOrValue::Token(Token::Ident(name)), Some(names)) => names.push(name.to_string()),
            (TokenOrValue::Token(Token::CloseSquareBracket), Some(_)) => target.extend(current.take()),
            (TokenOrValue::Function(function), None) if function.name.0.eq_ignore_ascii_case("repeat") => {
                let arguments = significant(&function.arguments);
                let (count, lists) = match arguments.as_slice() {
                    [count, TokenOrValue::Token(Token::Comma), lists @ ..] => (*count, lists),
                    _ => return None,
                };
                let lists = name_lists(lists)?;
                match count {
                    TokenOrValue::Token(Token::Ident(ident)) if ident.eq_ignore_ascii_case("auto-fill") => {
                        if lines.repeated.is_some() || lists.is_empty() {
                            return None;
                        }
                        lines.repeated = Some(lists);
                    }
                    TokenOrValue::Token(Token::Number {
                        int_value: Some(count),
                        ..
                    }) if *count > 0 => {
                        let count = usize::try_from(*count).ok()?;
                        let limit = MAX_REPEATED_LISTS / lists.len().max(1);
                        for _ in 0..count.min(limit) {
                            target.extend(lists.iter().cloned());
                        }
                    }
                    _ => return None,
                }
            }
            _ => return None,
        }
    }
    current.is_none().then_some(lines)
}

/// The rows and columns of a `grid` or `grid-template` shorthand of the form `rows / columns`
/// where either is `subgrid` (CSS Grid 2, §7.4), which lightningcss does not parse; `None`
/// for other values.
pub fn subgrid_shorthand_sides<'i>(tokens: &TokenList<'i>) -> Option<(TokenList<'i>, TokenList<'i>)> {
    let mut sides: Vec<Vec<TokenOrValue<'i>>> = vec![Vec::new()];
    for token in &tokens.0 {
        match token {
            TokenOrValue::Token(Token::Delim('/')) => sides.push(Vec::new()),
            other => sides.last_mut()?.push(other.clone()),
        }
    }
    let [rows, columns] = <[Vec<TokenOrValue<'i>>; 2]>::try_from(sides).ok()?;
    let (rows, columns) = (TokenList(rows), TokenList(columns));
    (subgrid_lines(&rows).is_some() || subgrid_lines(&columns).is_some()).then_some((rows, columns))
}

/// The lists of names `[…]` of a `repeat()` in a `subgrid` value.
fn name_lists(tokens: &[&TokenOrValue<'_>]) -> Option<Vec<Vec<String>>> {
    let mut lists = Vec::new();
    let mut current: Option<Vec<String>> = None;
    for token in tokens {
        match (token, &mut current) {
            (TokenOrValue::Token(Token::SquareBracketBlock), None) => current = Some(Vec::new()),
            (TokenOrValue::Token(Token::Ident(name)), Some(names)) => names.push(name.to_string()),
            (TokenOrValue::Token(Token::CloseSquareBracket), Some(_)) => lists.extend(current.take()),
            _ => return None,
        }
    }
    current.is_none().then_some(lists)
}

fn significant<'a, 'i>(tokens: &'a TokenList<'i>) -> Vec<&'a TokenOrValue<'i>> {
    tokens
        .0
        .iter()
        .filter(|token| !matches!(token, TokenOrValue::Token(Token::WhiteSpace(_))))
        .collect()
}

#[cfg(test)]
mod tests {
    use lightningcss::properties::Property;
    use lightningcss::stylesheet::ParserOptions;

    use super::*;

    fn position(value: &str) -> Option<String> {
        let property = Property::parse_string("position".into(), value, ParserOptions::default()).ok()?;
        match property {
            Property::Unparsed(unparsed) => running_name(&unparsed.value),
            _ => None,
        }
    }

    #[test]
    fn reads_running_names() {
        assert_eq!(position("running(header)").as_deref(), Some("header"));
        assert_eq!(position(" running( footer ) ").as_deref(), Some("footer"));
        assert_eq!(position("running(a, b)"), None);
        assert_eq!(position("running()"), None);
        assert_eq!(position("absolute"), None);
    }
}

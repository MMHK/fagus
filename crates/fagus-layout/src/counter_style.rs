//! Counter styles (CSS Counter Styles 3): the predefined ones and those of `@counter-style`
//! rules, and the text they give counter values, as in Chrome.
//!
//! The predefined styles (`counter_styles.css`) and the algorithms of the systems Chrome adds for
//! the complex ones (Hebrew, Armenian, Chinese, Korean and Ethiopic numbers) follow Blink's
//! `ua_counter_style_map.cc` and `counter_style.cc`, under their BSD license
//! (`CHROMIUM-LICENSE`).

use std::sync::{Arc, LazyLock};

use fagus_css::lightningcss::properties::Property;
use fagus_css::lightningcss::properties::custom::{CustomPropertyName, Token, TokenOrValue};
use fagus_css::lightningcss::rules::counter_style::CounterStyleRule;
use fagus_css::{MediaEnvironment, Origin, RuleSet, Stylesheet};
use rustc_hash::FxHashMap;

/// Representations of more symbols than this are left to the fallback style, as in Chrome.
const LENGTH_LIMIT: usize = 120;

/// The styles that `@counter-style` rules may not replace (CSS Counter Styles 3, §3).
const UNCHANGEABLE: [&str; 6] = [
    "decimal",
    "disc",
    "square",
    "circle",
    "disclosure-open",
    "disclosure-closed",
];

/// How a counter style turns a value into symbols (`system`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum System {
    Cyclic,
    /// The symbols stand for the values from this one on.
    Fixed(i32),
    Symbolic,
    Alphabetic,
    Numeric,
    Additive,
    Hebrew,
    Armenian {
        upper: bool,
    },
    Cjk(Cjk),
    EthiopicNumeric,
}

impl System {
    /// Whether the system shows the value of negative numbers, with the negative sign before.
    fn signed(self) -> bool {
        !matches!(self, Self::Cyclic | Self::Fixed(_))
    }

    /// The values the system can show, where `range` is `auto`.
    fn shows(self, value: i32) -> bool {
        match self {
            Self::Cyclic | Self::Numeric | Self::Fixed(_) | Self::Cjk(_) => true,
            Self::Symbolic | Self::Alphabetic | Self::EthiopicNumeric => value >= 1,
            Self::Additive => value >= 0,
            Self::Hebrew => (0..=999_999).contains(&value),
            Self::Armenian { .. } => (1..=99_999_999).contains(&value),
        }
    }
}

/// The numbers of the CJK languages, as Chrome writes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cjk {
    SimpChineseInformal,
    SimpChineseFormal,
    TradChineseInformal,
    TradChineseFormal,
    KoreanHangulFormal,
    KoreanHanjaInformal,
    KoreanHanjaFormal,
}

/// The `system` descriptor.
#[derive(Clone, Debug)]
enum Declared {
    System(System),
    Extends(String),
}

/// The `range` descriptor.
#[derive(Clone, Debug)]
enum Range {
    Auto,
    Bounds(Vec<(i64, i64)>),
}

/// The descriptors of an `@counter-style` rule, as given.
#[derive(Clone, Debug, Default)]
struct Descriptors {
    system: Option<Declared>,
    symbols: Option<Vec<String>>,
    additive: Option<Vec<(u32, String)>>,
    negative: Option<(String, String)>,
    prefix: Option<String>,
    suffix: Option<String>,
    range: Option<Range>,
    pad: Option<(usize, String)>,
    fallback: Option<String>,
}

/// A counter style, its `extends` resolved.
#[derive(Clone, Debug)]
struct Style {
    system: System,
    symbols: Vec<String>,
    /// The weights of the symbols of an additive style.
    weights: Vec<u32>,
    negative: (String, String),
    prefix: String,
    suffix: String,
    range: Range,
    pad: (usize, String),
    fallback: String,
}

/// The counter styles of a document: the predefined ones, and those its `@counter-style` rules
/// define or replace.
#[derive(Clone, Debug)]
pub struct CounterStyles {
    styles: Arc<FxHashMap<String, Style>>,
}

/// The predefined counter styles, as Chrome defines them.
static PREDEFINED: LazyLock<FxHashMap<String, Descriptors>> = LazyLock::new(|| {
    let sheet = Stylesheet::parse(
        include_str!("counter_styles.css"),
        Origin::UserAgent,
        "counter_styles.css",
    );
    let rules = RuleSet::new(&[&sheet], &MediaEnvironment::default());
    rules
        .counter_styles()
        .iter()
        .filter_map(|rule| Some((rule.name.0.to_string(), descriptors(rule, true)?)))
        .collect()
});

/// The predefined counter styles, resolved.
static RESOLVED: LazyLock<CounterStyles> = LazyLock::new(|| CounterStyles::from_rules(&[]));

impl Default for CounterStyles {
    fn default() -> Self {
        RESOLVED.clone()
    }
}

impl CounterStyles {
    /// The predefined counter styles and those of the `@counter-style` rules of `rules`.
    pub fn new(rules: &RuleSet) -> Self {
        match rules.counter_styles() {
            [] => Self::default(),
            rules => Self::from_rules(rules),
        }
    }

    fn from_rules(rules: &[CounterStyleRule<'static>]) -> Self {
        let mut declared = PREDEFINED.clone();
        for rule in rules {
            let name = rule.name.0.to_string();
            if UNCHANGEABLE.contains(&name.as_str()) {
                continue;
            }
            if let Some(descriptors) = descriptors(rule, false) {
                declared.insert(name, descriptors);
            }
        }
        let mut styles = FxHashMap::default();
        for name in declared.keys() {
            resolve(name, &declared, &mut styles, &mut Vec::new());
        }
        Self {
            styles: Arc::new(styles),
        }
    }

    /// The text of `value` in the counter style `name` (CSS Counter Styles 3, §3.1): its
    /// symbols, padded and signed, or those of the fallback style where the style cannot show
    /// it. A style that is not defined is `decimal`.
    pub fn text(&self, name: &str, value: i32) -> String {
        self.represent(name, value, &mut Vec::new())
    }

    /// The text of a list item's marker in the style `name`: the value between the style's
    /// prefix and suffix.
    pub fn marker(&self, name: &str, value: i32) -> String {
        let style = self.style(name);
        format!("{}{}{}", style.prefix, self.text(name, value), style.suffix)
    }

    fn style(&self, name: &str) -> &Style {
        self.styles
            .get(name)
            .or_else(|| self.styles.get(&name.to_ascii_lowercase()))
            .or_else(|| self.styles.get("decimal"))
            .expect("decimal is predefined")
    }

    fn represent(&self, name: &str, value: i32, tried: &mut Vec<String>) -> String {
        let style = self.style(name);
        let initial = (style.pad.0 <= LENGTH_LIMIT)
            .then(|| style.initial(value))
            .flatten();
        let Some(initial) = initial else {
            // The fallback style shows it, or decimal where the fallbacks go round in a circle.
            tried.push(name.to_owned());
            if tried.contains(&style.fallback) {
                return value.to_string();
            }
            let fallback = style.fallback.clone();
            return self.represent(&fallback, value, tried);
        };
        let negative = value < 0 && style.system.signed();
        let mut length = initial.chars().count();
        if negative {
            length += style.negative.0.chars().count() + style.negative.1.chars().count();
        }
        let mut text = String::new();
        if negative {
            text.push_str(&style.negative.0);
        }
        for _ in length..style.pad.0 {
            text.push_str(&style.pad.1);
        }
        text.push_str(&initial);
        if negative {
            text.push_str(&style.negative.1);
        }
        text
    }
}

impl Style {
    /// The symbols the style's system gives `value`, without sign or padding; `None` where it
    /// cannot show it.
    fn initial(&self, value: i32) -> Option<String> {
        let in_range = match &self.range {
            Range::Auto => self.system.shows(value),
            Range::Bounds(bounds) => bounds
                .iter()
                .any(|&(low, high)| (low..=high).contains(&i64::from(value))),
        };
        if !in_range {
            return None;
        }
        let absolute = value.unsigned_abs();
        let count = self.symbols.len();
        let indexes = match self.system {
            System::Cyclic => cyclic(value, count),
            System::Fixed(first) => fixed(value, first, count),
            System::Numeric => numeric(absolute, count),
            System::Symbolic => symbolic(absolute, count),
            System::Alphabetic => alphabetic(absolute, count),
            System::Additive => additive(absolute, &self.weights),
            System::Hebrew => return hebrew(absolute),
            System::Armenian { upper } => return armenian(absolute, upper),
            System::Cjk(cjk) => return Some(cjk_ideographic(absolute, cjk)),
            System::EthiopicNumeric => return ethiopic(absolute),
        }?;
        (!indexes.is_empty()).then(|| {
            indexes
                .iter()
                .map(|&index| self.symbols[index].as_str())
                .collect()
        })
    }
}

/// Resolves the style `name` and the styles it extends, as if a style in a circle of them
/// extended `decimal`.
fn resolve(
    name: &str,
    declared: &FxHashMap<String, Descriptors>,
    styles: &mut FxHashMap<String, Style>,
    chain: &mut Vec<String>,
) -> Style {
    if let Some(style) = styles.get(name) {
        return style.clone();
    }
    let Some(descriptors) = declared.get(name) else {
        return resolve("decimal", declared, styles, chain);
    };
    let style = match &descriptors.system {
        Some(Declared::Extends(base)) => {
            let base = if *base == name || chain.contains(base) || !declared.contains_key(base) {
                "decimal"
            } else {
                base
            };
            chain.push(name.to_owned());
            let base = resolve(base, declared, styles, chain);
            chain.pop();
            let descriptors = descriptors.clone();
            Style {
                negative: descriptors.negative.unwrap_or(base.negative),
                prefix: descriptors.prefix.unwrap_or(base.prefix),
                suffix: descriptors.suffix.unwrap_or(base.suffix),
                range: descriptors.range.unwrap_or(base.range),
                pad: descriptors.pad.unwrap_or(base.pad),
                fallback: descriptors.fallback.unwrap_or(base.fallback),
                ..base
            }
        }
        system => {
            let system = match system {
                Some(Declared::System(system)) => *system,
                _ => System::Symbolic,
            };
            let descriptors = descriptors.clone();
            let (weights, symbols) = match descriptors.additive {
                Some(additive) => additive.into_iter().unzip(),
                None => (Vec::new(), descriptors.symbols.unwrap_or_default()),
            };
            Style {
                system,
                symbols,
                weights,
                negative: descriptors
                    .negative
                    .unwrap_or_else(|| ("-".to_owned(), String::new())),
                prefix: descriptors.prefix.unwrap_or_default(),
                suffix: descriptors.suffix.unwrap_or_else(|| ". ".to_owned()),
                range: descriptors.range.unwrap_or(Range::Auto),
                pad: descriptors.pad.unwrap_or((0, String::new())),
                fallback: descriptors.fallback.unwrap_or_else(|| "decimal".to_owned()),
            }
        }
    };
    styles.insert(name.to_owned(), style.clone());
    style
}

/// The descriptors of a rule, if it is valid: its system has the symbols it needs, and only
/// the predefined styles use Chrome's own systems (`internal`).
fn descriptors(rule: &CounterStyleRule<'static>, internal: bool) -> Option<Descriptors> {
    let mut descriptors = Descriptors::default();
    for property in &rule.declarations.declarations {
        let Property::Custom(custom) = property else {
            continue;
        };
        let CustomPropertyName::Unknown(name) = &custom.name else {
            continue;
        };
        let tokens: Vec<&TokenOrValue<'static>> = custom
            .value
            .0
            .iter()
            .filter(|token| !matches!(token, TokenOrValue::Token(Token::WhiteSpace(_))))
            .collect();
        // An invalid descriptor is dropped; the rest of the rule holds.
        match name.0.to_ascii_lowercase().as_str() {
            "system" => descriptors.system = system(&tokens, internal).or(descriptors.system),
            "symbols" => {
                let symbols: Option<Vec<String>> = tokens.iter().map(|token| symbol(token)).collect();
                descriptors.symbols = symbols
                    .filter(|symbols| !symbols.is_empty())
                    .or(descriptors.symbols);
            }
            "additive-symbols" => descriptors.additive = additive_symbols(&tokens).or(descriptors.additive),
            "negative" => {
                descriptors.negative = match tokens.as_slice() {
                    [before] => symbol(before).map(|before| (before, String::new())),
                    [before, after] => symbol(before).zip(symbol(after)),
                    _ => None,
                }
                .or(descriptors.negative);
            }
            "prefix" => descriptors.prefix = single(&tokens).or(descriptors.prefix),
            "suffix" => descriptors.suffix = single(&tokens).or(descriptors.suffix),
            "range" => descriptors.range = range(&tokens).or(descriptors.range),
            "pad" => {
                descriptors.pad = match tokens.as_slice() {
                    [first, second] => integer(first)
                        .zip(symbol(second))
                        .or_else(|| integer(second).zip(symbol(first)))
                        .and_then(|(length, symbol)| Some((usize::try_from(length).ok()?, symbol))),
                    _ => None,
                }
                .or(descriptors.pad);
            }
            "fallback" => {
                descriptors.fallback = match tokens.as_slice() {
                    [TokenOrValue::Token(Token::Ident(name))] => Some(name.to_string()),
                    _ => None,
                }
                .or(descriptors.fallback);
            }
            _ => {}
        }
    }
    let valid = match &descriptors.system {
        // An extending style takes the symbols of the one it extends.
        Some(Declared::Extends(_)) => descriptors.symbols.is_none() && descriptors.additive.is_none(),
        Some(Declared::System(System::Additive)) => descriptors.additive.is_some(),
        Some(Declared::System(System::Alphabetic | System::Numeric)) => descriptors
            .symbols
            .as_ref()
            .is_some_and(|symbols| symbols.len() >= 2),
        Some(Declared::System(System::Cyclic | System::Fixed(_) | System::Symbolic)) | None => {
            descriptors.symbols.is_some()
        }
        Some(Declared::System(_)) => true,
    };
    valid.then_some(descriptors)
}

/// Parses `system`.
fn system(tokens: &[&TokenOrValue<'static>], internal: bool) -> Option<Declared> {
    let (first, rest) = tokens.split_first()?;
    let TokenOrValue::Token(Token::Ident(keyword)) = first else {
        return None;
    };
    let keyword = keyword.to_ascii_lowercase();
    let simple = match keyword.as_str() {
        "cyclic" => Some(System::Cyclic),
        "symbolic" => Some(System::Symbolic),
        "alphabetic" => Some(System::Alphabetic),
        "numeric" => Some(System::Numeric),
        "additive" => Some(System::Additive),
        _ if !internal => None,
        "-internal-hebrew" => Some(System::Hebrew),
        "-internal-lower-armenian" => Some(System::Armenian { upper: false }),
        "-internal-upper-armenian" => Some(System::Armenian { upper: true }),
        "-internal-simp-chinese-informal" => Some(System::Cjk(Cjk::SimpChineseInformal)),
        "-internal-simp-chinese-formal" => Some(System::Cjk(Cjk::SimpChineseFormal)),
        "-internal-trad-chinese-informal" => Some(System::Cjk(Cjk::TradChineseInformal)),
        "-internal-trad-chinese-formal" => Some(System::Cjk(Cjk::TradChineseFormal)),
        "-internal-korean-hangul-formal" => Some(System::Cjk(Cjk::KoreanHangulFormal)),
        "-internal-korean-hanja-informal" => Some(System::Cjk(Cjk::KoreanHanjaInformal)),
        "-internal-korean-hanja-formal" => Some(System::Cjk(Cjk::KoreanHanjaFormal)),
        "-internal-ethiopic-numeric" => Some(System::EthiopicNumeric),
        _ => None,
    };
    if let Some(system) = simple {
        return rest.is_empty().then_some(Declared::System(system));
    }
    match (keyword.as_str(), rest) {
        ("fixed", []) => Some(Declared::System(System::Fixed(1))),
        ("fixed", [first]) => Some(Declared::System(System::Fixed(integer(first)?))),
        ("extends", [TokenOrValue::Token(Token::Ident(name))]) => Some(Declared::Extends(name.to_string())),
        _ => None,
    }
}

/// Parses `additive-symbols`: weights and symbols, the weights falling.
fn additive_symbols(tokens: &[&TokenOrValue<'static>]) -> Option<Vec<(u32, String)>> {
    let mut pairs = Vec::new();
    for pair in tokens.split(|token| matches!(token, TokenOrValue::Token(Token::Comma))) {
        let (weight, symbol) = match pair {
            [first, second] => integer(first)
                .zip(symbol(second))
                .or_else(|| integer(second).zip(symbol(first)))?,
            _ => return None,
        };
        let weight = u32::try_from(weight).ok()?;
        if pairs.last().is_some_and(|&(last, _)| weight >= last) {
            return None;
        }
        pairs.push((weight, symbol));
    }
    (!pairs.is_empty()).then_some(pairs)
}

/// Parses `range`: `auto`, or pairs of bounds, each an integer or `infinite`.
fn range(tokens: &[&TokenOrValue<'static>]) -> Option<Range> {
    if let [TokenOrValue::Token(Token::Ident(keyword))] = tokens
        && keyword.eq_ignore_ascii_case("auto")
    {
        return Some(Range::Auto);
    }
    let bound = |token: &TokenOrValue<'static>, infinite: i64| match token {
        TokenOrValue::Token(Token::Ident(keyword)) if keyword.eq_ignore_ascii_case("infinite") => {
            Some(infinite)
        }
        token => integer(token).map(i64::from),
    };
    let mut bounds = Vec::new();
    for pair in tokens.split(|token| matches!(token, TokenOrValue::Token(Token::Comma))) {
        let [low, high] = pair else { return None };
        let (low, high) = (bound(low, i64::MIN)?, bound(high, i64::MAX)?);
        if low > high {
            return None;
        }
        bounds.push((low, high));
    }
    (!bounds.is_empty()).then_some(Range::Bounds(bounds))
}

/// A descriptor of one symbol.
fn single(tokens: &[&TokenOrValue<'static>]) -> Option<String> {
    match tokens {
        [token] => symbol(token),
        _ => None,
    }
}

/// A symbol: a string or an identifier.
fn symbol(token: &TokenOrValue<'static>) -> Option<String> {
    match token {
        TokenOrValue::Token(Token::String(text) | Token::Ident(text)) => Some(text.to_string()),
        _ => None,
    }
}

/// An integer: a whole number, or a math function giving a number, rounded to the nearest
/// integer (`calc(1 + sign(100em - 1px))`).
fn integer(token: &TokenOrValue<'static>) -> Option<i32> {
    match token {
        TokenOrValue::Token(Token::Number {
            int_value: Some(value),
            ..
        }) => Some(*value),
        TokenOrValue::Function(function) => {
            let value = fagus_style::descriptor_number(function)?.round();
            #[expect(clippy::cast_possible_truncation, reason = "clamped to the range of i32")]
            Some(value.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32)
        }
        _ => None,
    }
}

/// The symbol of a value of a cyclic style.
fn cyclic(value: i32, count: usize) -> Option<Vec<usize>> {
    let count = i32::try_from(count).ok().filter(|count| *count > 0)?;
    let index = (value % count - 1).rem_euclid(count);
    Some(vec![usize::try_from(index).ok()?])
}

/// The symbol of a value of a fixed style, whose symbols start at `first`.
fn fixed(value: i32, first: i32, count: usize) -> Option<Vec<usize>> {
    let index = usize::try_from(i64::from(value) - i64::from(first)).ok()?;
    (index < count).then(|| vec![index])
}

/// A symbolic style's symbols: the value's symbol, repeated once for each time round.
fn symbolic(value: u32, count: usize) -> Option<Vec<usize>> {
    if value == 0 || count == 0 {
        return None;
    }
    let value = value as usize;
    let repetitions = value.div_ceil(count);
    (repetitions <= LENGTH_LIMIT).then(|| vec![(value - 1) % count; repetitions])
}

/// An alphabetic style's symbols: the value in bijective base `count`.
fn alphabetic(mut value: u32, count: usize) -> Option<Vec<usize>> {
    let count = u32::try_from(count).ok().filter(|count| *count > 1)?;
    if value == 0 {
        return None;
    }
    let mut indexes = Vec::new();
    while value > 0 {
        value -= 1;
        indexes.push((value % count) as usize);
        value /= count;
    }
    indexes.reverse();
    Some(indexes)
}

/// A numeric style's symbols: the value in base `count`.
fn numeric(mut value: u32, count: usize) -> Option<Vec<usize>> {
    let count = u32::try_from(count).ok().filter(|count| *count > 1)?;
    if value == 0 {
        return Some(vec![0]);
    }
    let mut indexes = Vec::new();
    while value > 0 {
        indexes.push((value % count) as usize);
        value /= count;
    }
    indexes.reverse();
    Some(indexes)
}

/// An additive style's symbols: the largest weights first, as often as they go in.
fn additive(mut value: u32, weights: &[u32]) -> Option<Vec<usize>> {
    if value == 0 {
        return (weights.last() == Some(&0)).then(|| vec![weights.len() - 1]);
    }
    let mut indexes = Vec::new();
    for (index, &weight) in weights.iter().enumerate() {
        if value == 0 || weight == 0 {
            break;
        }
        let repetitions = (value / weight) as usize;
        if indexes.len() + repetitions > LENGTH_LIMIT {
            return None;
        }
        indexes.extend(std::iter::repeat_n(index, repetitions));
        value %= weight;
    }
    (value == 0).then_some(indexes)
}

/// A Hebrew number below 1000, as Blink writes it.
fn hebrew_below_1000(mut number: u32) -> String {
    let letter = |code: u32| char::from_u32(code).unwrap_or('\u{FFFD}');
    let mut letters = String::new();
    for _ in 0..number / 400 {
        letters.push(letter(1511 + 3));
    }
    number %= 400;
    if number / 100 > 0 {
        letters.push(letter(1511 + number / 100 - 1));
    }
    number %= 100;
    // 15 and 16 are written 9 + 6 and 9 + 7, so as not to spell a name of God.
    if number == 15 || number == 16 {
        letters.push(letter(1487 + 9));
        letters.push(letter(1487 + number - 9));
    } else {
        const TENS: [u32; 9] = [1497, 1499, 1500, 1502, 1504, 1505, 1506, 1508, 1510];
        if number / 10 > 0 {
            letters.push(letter(TENS[(number / 10 - 1) as usize]));
        }
        if !number.is_multiple_of(10) {
            letters.push(letter(1487 + number % 10));
        }
    }
    letters
}

/// A Hebrew number, the thousands marked with a geresh.
fn hebrew(number: u32) -> Option<String> {
    match number {
        1_000_000.. => None,
        0 => Some("\u{05D0}\u{05E4}\u{05E1}".to_owned()),
        1..=999 => Some(hebrew_below_1000(number)),
        _ => Some(format!(
            "{}\u{05F3}{}",
            hebrew_below_1000(number / 1000),
            hebrew_below_1000(number % 1000)
        )),
    }
}

/// An Armenian number below 10,000; a circumflex over each letter multiplies it by 10,000.
fn armenian_below_10000(number: u32, upper: bool, circumflex: bool) -> String {
    let offset = if upper { 0 } else { 0x30 };
    let mut letters = String::new();
    let mut push = |code: u32| {
        letters.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
        if circumflex {
            letters.push('\u{0302}');
        }
    };
    match number / 1000 {
        0 => {}
        7 => push(0x0552 + offset),
        thousands => push(0x054C - 1 + offset + thousands),
    }
    let hundreds = (number / 100) % 10;
    if hundreds > 0 {
        push(0x0543 - 1 + offset + hundreds);
    }
    let tens = (number / 10) % 10;
    if tens > 0 {
        push(0x053A - 1 + offset + tens);
    }
    let ones = number % 10;
    if ones > 0 {
        push(0x0531 - 1 + offset + ones);
    }
    letters
}

/// An Armenian number, up to 99,999,999.
fn armenian(number: u32, upper: bool) -> Option<String> {
    if number == 0 || number > 99_999_999 {
        return None;
    }
    Some(format!(
        "{}{}",
        armenian_below_10000(number / 10000, upper, true),
        armenian_below_10000(number % 10000, upper, false)
    ))
}

/// An Ethiopic number (CSS Counter Styles 3, §7.2): pairs of digits joined by the signs for
/// hundred and ten thousand.
fn ethiopic(mut value: u32) -> Option<String> {
    const UNITS: [u16; 9] = [
        0x1369, 0x136A, 0x136B, 0x136C, 0x136D, 0x136E, 0x136F, 0x1370, 0x1371,
    ];
    const TENS: [u16; 9] = [
        0x1372, 0x1373, 0x1374, 0x1375, 0x1376, 0x1377, 0x1378, 0x1379, 0x137A,
    ];
    const HUNDRED: u16 = 0x137B;
    const TEN_THOUSAND: u16 = 0x137C;
    if value == 0 {
        return None;
    }
    if value < 10 {
        return Some(String::from_utf16_lossy(&[UNITS[value as usize - 1]]));
    }
    // From the last pair on, backwards.
    let mut reversed = Vec::new();
    let mut odd = false;
    while value > 0 {
        let group = value % 100;
        value /= 100;
        if !odd {
            // One too many for the last pair, removed at the end.
            reversed.push(TEN_THOUSAND);
        } else if group > 0 {
            reversed.push(HUNDRED);
        }
        let most_significant = value == 0;
        let without_digits = group == 0 || (group == 1 && (most_significant || odd));
        if !without_digits {
            if !group.is_multiple_of(10) {
                reversed.push(UNITS[(group % 10 - 1) as usize]);
            }
            if group / 10 > 0 {
                reversed.push(TENS[(group / 10 - 1) as usize]);
            }
        }
        odd = !odd;
    }
    reversed.reverse();
    reversed.pop();
    Some(String::from_utf16_lossy(&reversed))
}

/// A number in a CJK language (CSS Counter Styles 3, §7.1), as Blink writes it: groups of four
/// digits, each digit followed by the marker of its place, zeros left out or written once.
fn cjk_ideographic(mut number: u32, cjk: Cjk) -> String {
    // The table: the language, three group markers of two characters, three digit markers and
    // ten digits.
    const CHINESE: u16 = 1;
    const KOREAN: u16 = 2;
    const NONE: u8 = 0;
    const SECOND_GROUP: u8 = 1;
    const THIRD_GROUP: u8 = 3;
    const FOURTH_GROUP: u8 = 5;
    const SECOND_DIGIT: u8 = 7;
    const THIRD_DIGIT: u8 = 8;
    const FOURTH_DIGIT: u8 = 9;
    const DIGIT0: u8 = 10;
    let (table, formal): ([u16; 21], bool) = match cjk {
        Cjk::SimpChineseInformal => (
            [
                CHINESE, 0x4E07, 0, 0x4EBF, 0, 0x4E07, 0x4EBF, 0x5341, 0x767E, 0x5343, 0x96F6, 0x4E00,
                0x4E8C, 0x4E09, 0x56DB, 0x4E94, 0x516D, 0x4E03, 0x516B, 0x4E5D, 0,
            ],
            false,
        ),
        Cjk::SimpChineseFormal => (
            [
                CHINESE, 0x4E07, 0, 0x4EBF, 0, 0x4E07, 0x4EBF, 0x62FE, 0x4F70, 0x4EDF, 0x96F6, 0x58F9,
                0x8D30, 0x53C1, 0x8086, 0x4F0D, 0x9646, 0x67D2, 0x634C, 0x7396, 0,
            ],
            true,
        ),
        Cjk::TradChineseInformal => (
            [
                CHINESE, 0x842C, 0, 0x5104, 0, 0x5146, 0, 0x5341, 0x767E, 0x5343, 0x96F6, 0x4E00, 0x4E8C,
                0x4E09, 0x56DB, 0x4E94, 0x516D, 0x4E03, 0x516B, 0x4E5D, 0,
            ],
            false,
        ),
        Cjk::TradChineseFormal => (
            [
                CHINESE, 0x842C, 0, 0x5104, 0, 0x5146, 0, 0x62FE, 0x4F70, 0x4EDF, 0x96F6, 0x58F9, 0x8CB3,
                0x53C3, 0x8086, 0x4F0D, 0x9678, 0x67D2, 0x634C, 0x7396, 0,
            ],
            true,
        ),
        Cjk::KoreanHangulFormal => (
            [
                KOREAN, 0xB9CC, 0, 0xC5B5, 0, 0xC870, 0, 0xC2ED, 0xBC31, 0xCC9C, 0xC601, 0xC77C, 0xC774,
                0xC0BC, 0xC0AC, 0xC624, 0xC721, 0xCE60, 0xD314, 0xAD6C, 0,
            ],
            true,
        ),
        Cjk::KoreanHanjaInformal => (
            [
                KOREAN, 0x842C, 0, 0x5104, 0, 0x5146, 0, 0x5341, 0x767E, 0x5343, 0x96F6, 0x4E00, 0x4E8C,
                0x4E09, 0x56DB, 0x4E94, 0x516D, 0x4E03, 0x516B, 0x4E5D, 0,
            ],
            false,
        ),
        Cjk::KoreanHanjaFormal => (
            [
                KOREAN, 0x842C, 0, 0x5104, 0, 0x5146, 0, 0x62FE, 0x767E, 0x4EDF, 0x96F6, 0x58F9, 0x8CB3,
                0x53C3, 0x56DB, 0x4E94, 0x516D, 0x4E03, 0x516B, 0x4E5D, 0,
            ],
            true,
        ),
    };
    let (chinese, korean) = (table[0] == CHINESE, table[0] == KOREAN);
    if number == 0 {
        return String::from_utf16_lossy(&[table[DIGIT0 as usize]]);
    }
    // Four groups of four digits, three digit markers and a group marker of two characters,
    // the last group first.
    let mut buffer = [NONE; 36];
    let digit = |value: u32| DIGIT0 + value as u8;
    for i in 0..4u8 {
        let group_value = number % 10000;
        number /= 10000;
        let start = usize::from(3 - i) * 9;
        let group = &mut buffer[start..start + 9];
        if group_value > 0 && i > 0 {
            group[8] = SECOND_GROUP + i;
            group[7] = SECOND_GROUP - 1 + i;
        }
        // Korean informal numbers leave out the one before a marker.
        let drops_one = |value: u32| korean && !formal && value == 1;
        let mut value = group_value % 10;
        let mut trailing_zero = chinese && value == 0;
        if value > 0 && !(drops_one(value) && i > 0) {
            group[6] = digit(value);
        }
        if number != 0 || group_value > 9 {
            value = (group_value / 10) % 10;
            if (value > 0 && !drops_one(value)) || (value == 0 && !trailing_zero) {
                group[4] = digit(value);
            }
            trailing_zero &= value == 0;
            if value > 0 {
                group[5] = SECOND_DIGIT;
            }
        }
        if number != 0 || group_value > 99 {
            value = (group_value / 100) % 10;
            if (value > 0 && !drops_one(value)) || (value == 0 && !trailing_zero) {
                group[2] = digit(value);
            }
            trailing_zero &= value == 0;
            if value > 0 {
                group[3] = THIRD_DIGIT;
            }
        }
        if number != 0 || group_value > 999 {
            value = group_value / 1000;
            if (value > 0 && !drops_one(value)) || (value == 0 && !trailing_zero) {
                group[0] = digit(value);
            }
            if value > 0 {
                group[1] = FOURTH_DIGIT;
            }
        }
        if trailing_zero && i > 0 {
            group[6] = group[7];
            group[7] = group[8];
            group[8] = DIGIT0;
        }
        // Informal Chinese leaves out the one of the tens below twenty, but keeps its marker.
        if chinese && !formal && group_value < 20 {
            group[4] = NONE;
        }
        if number == 0 {
            break;
        }
    }
    // Into characters, leaving out runs of zeros and a zero at the end.
    let mut characters: Vec<u16> = Vec::new();
    let mut last = NONE;
    for &abstract_char in &buffer {
        if abstract_char == NONE {
            continue;
        }
        if abstract_char != DIGIT0 || (chinese && last != DIGIT0) {
            let character = table[usize::from(abstract_char)];
            if character != 0 {
                characters.push(character);
                if korean && matches!(abstract_char, SECOND_GROUP | THIRD_GROUP | FOURTH_GROUP) {
                    characters.push(u16::from(b' '));
                }
            }
        }
        last = abstract_char;
    }
    if (chinese && last == DIGIT0) || characters.last() == Some(&u16::from(b' ')) {
        characters.pop();
    }
    String::from_utf16_lossy(&characters)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn styles(css: &str) -> CounterStyles {
        let sheet = Stylesheet::parse(css, Origin::Author, "style");
        CounterStyles::new(&RuleSet::new(&[&sheet], &MediaEnvironment::default()))
    }

    /// The texts of `values` in the style `name`, as `[a][b]…`.
    fn texts(styles: &CounterStyles, name: &str, values: &[i32]) -> String {
        let mut out = String::new();
        for &value in values {
            out.push('[');
            out.push_str(&styles.text(name, value));
            out.push(']');
        }
        out
    }

    const VALUES: [i32; 15] = [-3, 0, 1, 2, 3, 4, 5, 9, 10, 14, 26, 27, 99, 1999, 4000];

    #[test]
    fn writes_the_predefined_styles_as_chrome() {
        let styles = CounterStyles::default();
        // Each as Chrome 141 prints `counter(c, <style>)` for these values.
        for (name, expected) in [
            (
                "decimal-leading-zero",
                "[-3][00][01][02][03][04][05][09][10][14][26][27][99][1999][4000]",
            ),
            (
                "upper-roman",
                "[-3][0][I][II][III][IV][V][IX][X][XIV][XXVI][XXVII][XCIX][MCMXCIX][4000]",
            ),
            (
                "lower-alpha",
                "[-3][0][a][b][c][d][e][i][j][n][z][aa][cu][bxw][ewv]",
            ),
            (
                "lower-greek",
                "[-3][0][α][β][γ][δ][ε][ι][κ][ξ][αβ][αγ][δγ][γλη][ζχπ]",
            ),
            (
                "armenian",
                "[-3][0][Ա][Բ][Գ][Դ][Ե][Թ][Ժ][ԺԴ][ԻԶ][ԻԷ][ՂԹ][ՌՋՂԹ][Տ]",
            ),
            (
                "lower-armenian",
                "[-3][0][ա][բ][գ][դ][ե][թ][ժ][ժդ][իզ][իէ][ղթ][ռջղթ][տ]",
            ),
            (
                "georgian",
                "[-3][0][ა][ბ][გ][დ][ე][თ][ი][იდ][კვ][კზ][ჟთ][ჩშჟთ][წ]",
            ),
            (
                "hebrew",
                "[-3][אפס][א][ב][ג][ד][ה][ט][י][יד][כו][כז][צט][א׳תתקצט][ד׳]",
            ),
            (
                "cjk-decimal",
                "[-3][〇][一][二][三][四][五][九][一〇][一四][二六][二七][九九][一九九九][四〇〇〇]",
            ),
            (
                "hiragana",
                "[-3][0][あ][い][う][え][お][け][こ][せ][は][ひ][いう][るま][あもた]",
            ),
            (
                "arabic-indic",
                "[-٣][٠][١][٢][٣][٤][٥][٩][١٠][١٤][٢٦][٢٧][٩٩][١٩٩٩][٤٠٠٠]",
            ),
            (
                "ethiopic-numeric",
                "[-3][0][፩][፪][፫][፬][፭][፱][፲][፲፬][፳፮][፳፯][፺፱][፲፱፻፺፱][፵፻]",
            ),
            (
                "trad-chinese-informal",
                "[負三][零][一][二][三][四][五][九][十][十四][二十六][二十七][九十九][一千九百九十九][四千]",
            ),
            (
                "unknown-style",
                "[-3][0][1][2][3][4][5][9][10][14][26][27][99][1999][4000]",
            ),
        ] {
            assert_eq!(texts(&styles, name, &VALUES), expected, "{name}");
        }
        assert_eq!(styles.marker("lower-greek", 24), "ω. ");
        assert_eq!(styles.marker("disc", 1), "\u{2022} ");
        assert_eq!(styles.marker("cjk-decimal", 3), "三、");
    }

    #[test]
    fn evaluates_math_functions_in_descriptors() {
        // As WPT's `descriptor-calc.html`: `sign(100em - 1px)` is 1.
        let styles = styles(
            r"@counter-style a { system: extends upper-roman;
                 range: calc(2 - sign(100em - 1px)) calc(5 + sign(100em - 1px));
                 pad: calc(3 + sign(100em - 1px)) '*' }
               @counter-style b { system: fixed calc(1 + sign(100em - 1px)); symbols: g h }
               @counter-style c { system: additive;
                 additive-symbols: calc(2 + sign(100em - 1px)) c, 2 b, 1 a }",
        );
        assert_eq!(texts(&styles, "a", &[1, 4, 6, 7]), "[***I][**IV][**VI][7]");
        assert_eq!(texts(&styles, "b", &[1, 2, 3, 4]), "[1][g][h][4]");
        assert_eq!(texts(&styles, "c", &[1, 3, 4, 7]), "[a][c][ca][cca]");
    }

    #[test]
    fn defines_styles_with_counter_style_rules() {
        let styles = styles(
            r#"@counter-style stern { system: cyclic; symbols: "*" "†" "‡"; suffix: " " }
               @counter-style feste { system: fixed 3; symbols: Ⓐ Ⓑ Ⓒ; suffix: ") " }
               @counter-style sym { system: symbolic; symbols: "*" "†"; }
               @counter-style alpha2 { system: alphabetic; symbols: x y z; prefix: "("; suffix: ") " }
               @counter-style bin { system: numeric; symbols: "0" "1"; pad: 4 "0"; negative: "−" }
               @counter-style rom { system: additive; range: 1 20; fallback: upper-alpha;
                 additive-symbols: 10 X, 9 IX, 5 V, 4 IV, 1 I }
               @counter-style ext { system: extends decimal; prefix: "Nr. "; suffix: ": " }
               @counter-style lower-roman { system: extends decimal; prefix: "!" }
               @counter-style klammer { system: extends lower-roman; suffix: ") " }
               @counter-style decimal { system: cyclic; symbols: x }
               @counter-style kaputt { system: alphabetic; symbols: x }"#,
        );
        // As Chrome 141 prints them.
        assert_eq!(
            texts(&styles, "stern", &VALUES),
            "[‡][‡][*][†][‡][*][†][‡][*][†][†][‡][‡][*][*]"
        );
        assert_eq!(
            texts(&styles, "feste", &VALUES),
            "[-3][0][1][2][Ⓐ][Ⓑ][Ⓒ][9][10][14][26][27][99][1999][4000]"
        );
        assert_eq!(
            texts(&styles, "sym", &[1, 2, 3, 4, 5, 9, 1999]),
            "[*][†][**][††][***][*****][1999]"
        );
        assert_eq!(
            texts(&styles, "alpha2", &VALUES),
            "[-3][0][x][y][z][xx][xy][yz][zx][xxy][yyy][yyz][zxyz][yxzxyzx][xxzzzzxx]"
        );
        assert_eq!(
            texts(&styles, "bin", &[-3, 0, 1, 5, 27]),
            "[−011][0000][0001][0101][11011]"
        );
        assert_eq!(
            texts(&styles, "rom", &[-3, 0, 1, 4, 9, 14, 26, 27]),
            "[-3][0][I][IV][IX][XIV][Z][AA]"
        );
        // A predefined style but decimal may be replaced; prefixes and suffixes are the
        // marker's, and an invalid rule is left out.
        assert_eq!(texts(&styles, "lower-roman", &[4]), "[4]");
        assert_eq!(texts(&styles, "klammer", &[4]), "[4]");
        assert_eq!(texts(&styles, "decimal", &[4]), "[4]");
        assert_eq!(texts(&styles, "kaputt", &[4]), "[4]");
        assert_eq!(styles.marker("ext", 1), "Nr. 1: ");
        assert_eq!(styles.marker("alpha2", 1), "(x) ");
        assert_eq!(styles.marker("stern", 2), "† ");
        assert_eq!(styles.marker("rom", 26), "Z. ");
    }
}

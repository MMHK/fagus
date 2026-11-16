//! The regular expressions of the `pattern` attribute (HTML §4.10.5.3.6): JavaScript patterns
//! compiled with the `v` flag, which must match a value as a whole. A pattern Fagus cannot read
//! (or that the `v` flag makes invalid, as an unescaped `-` at the end of a class) sets no
//! constraint, as an invalid one does in Chrome.
//!
//! Supported: literals, `.`, classes with ranges and the escapes `\d \w \s` (and their
//! negations), groups (capturing, `(?:…)`, `(?=…)`, `(?!…)`), alternatives, the quantifiers
//! `* + ? {n} {n,} {n,m}` (greedy or lazy), `^` and `$`.

/// A compiled pattern.
pub(crate) struct Pattern {
    alternatives: Alternatives,
}

type Alternatives = Vec<Vec<Node>>;

enum Node {
    Char(char),
    /// `.`: any character but a line terminator.
    Any,
    Class {
        negated: bool,
        items: Vec<ClassItem>,
    },
    Group(Alternatives),
    Look {
        negated: bool,
        alternatives: Alternatives,
    },
    Repeat {
        node: Box<Node>,
        min: u32,
        max: Option<u32>,
    },
    Start,
    End,
}

#[derive(Clone, Copy)]
enum ClassItem {
    Range(char, char),
    Digit(bool),
    Word(bool),
    Space(bool),
}

impl ClassItem {
    fn matches(self, c: char) -> bool {
        match self {
            Self::Range(low, high) => (low..=high).contains(&c),
            Self::Digit(negated) => c.is_ascii_digit() != negated,
            Self::Word(negated) => (c.is_ascii_alphanumeric() || c == '_') != negated,
            Self::Space(negated) => is_space(c) != negated,
        }
    }
}

/// White space and line terminators as JavaScript's `\s` has them.
fn is_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// The most steps a match may take before Fagus gives up on it (and sets no constraint).
const BUDGET: usize = 200_000;

impl Pattern {
    /// Compiles `source`, or `None` where it is not a pattern Fagus reads.
    pub(crate) fn parse(source: &str) -> Option<Self> {
        let chars: Vec<char> = source.chars().collect();
        let mut parser = Parser { chars: &chars, at: 0 };
        let alternatives = parser.alternatives()?;
        (parser.at == chars.len()).then_some(Self { alternatives })
    }

    /// Whether the pattern matches all of `text`; `None` where matching takes too long.
    pub(crate) fn matches_whole(&self, text: &str) -> Option<bool> {
        let chars: Vec<char> = text.chars().collect();
        let mut matcher = Matcher {
            text: &chars,
            steps: 0,
        };
        let whole = matcher.alternatives(&self.alternatives, 0, &mut |end, _| end == chars.len());
        (matcher.steps <= BUDGET).then_some(whole)
    }
}

struct Parser<'a> {
    chars: &'a [char],
    at: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn alternatives(&mut self) -> Option<Alternatives> {
        let mut alternatives = vec![self.sequence()?];
        while self.eat('|') {
            alternatives.push(self.sequence()?);
        }
        Some(alternatives)
    }

    fn sequence(&mut self) -> Option<Vec<Node>> {
        let mut nodes = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            let atom = self.atom()?;
            let node = self.quantified(atom)?;
            nodes.push(node);
        }
        Some(nodes)
    }

    fn atom(&mut self) -> Option<Node> {
        let c = self.peek()?;
        self.at += 1;
        Some(match c {
            '.' => Node::Any,
            '^' => Node::Start,
            '$' => Node::End,
            '(' => {
                let look = if self.eat('?') {
                    if self.eat(':') {
                        None
                    } else if self.eat('=') {
                        Some(false)
                    } else if self.eat('!') {
                        Some(true)
                    } else {
                        return None;
                    }
                } else {
                    None
                };
                let alternatives = self.alternatives()?;
                if !self.eat(')') {
                    return None;
                }
                match look {
                    Some(negated) => Node::Look {
                        negated,
                        alternatives,
                    },
                    None => Node::Group(alternatives),
                }
            }
            '[' => self.class()?,
            '\\' => match self.escape(false)? {
                Escaped::Char(c) => Node::Char(c),
                Escaped::Item(item) => Node::Class {
                    negated: false,
                    items: vec![item],
                },
            },
            // Quantifiers without an atom and lone brackets are syntax errors with the `u` and
            // `v` flags.
            '*' | '+' | '?' | '{' | '}' | ']' | ')' => return None,
            c => Node::Char(c),
        })
    }

    fn quantified(&mut self, atom: Node) -> Option<Node> {
        let (min, max) = match self.peek() {
            Some('*') => (0, None),
            Some('+') => (1, None),
            Some('?') => (0, Some(1)),
            Some('{') => {
                self.at += 1;
                let min = self.number()?;
                let max = if self.eat(',') {
                    if self.peek() == Some('}') {
                        None
                    } else {
                        Some(self.number()?)
                    }
                } else {
                    Some(min)
                };
                if !self.eat('}') || max.is_some_and(|max| max < min) {
                    return None;
                }
                self.at -= 1;
                (min, max)
            }
            _ => return Some(atom),
        };
        self.at += 1;
        // Lazy or greedy, a whole match exists or not alike.
        self.eat('?');
        if matches!(atom, Node::Start | Node::End | Node::Look { .. }) {
            return None;
        }
        Some(Node::Repeat {
            node: Box::new(atom),
            min,
            max,
        })
    }

    fn number(&mut self) -> Option<u32> {
        let start = self.at;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.at += 1;
        }
        self.chars[start..self.at].iter().collect::<String>().parse().ok()
    }

    /// A class after its `[`, as the `v` flag reads it (without set operations).
    fn class(&mut self) -> Option<Node> {
        let negated = self.eat('^');
        let mut items = Vec::new();
        loop {
            let c = self.peek()?;
            self.at += 1;
            let low = match c {
                ']' => break,
                '\\' => match self.escape(true)? {
                    Escaped::Char(c) => c,
                    Escaped::Item(item) => {
                        items.push(item);
                        continue;
                    }
                },
                // Syntax characters of a class must be escaped with the `v` flag.
                '(' | ')' | '[' | '{' | '}' | '/' | '-' | '|' => return None,
                c => c,
            };
            if self.peek() == Some('-') {
                self.at += 1;
                let high = match self.peek()? {
                    '\\' => {
                        self.at += 1;
                        match self.escape(true)? {
                            Escaped::Char(c) => c,
                            Escaped::Item(_) => return None,
                        }
                    }
                    '(' | ')' | '[' | ']' | '{' | '}' | '/' | '-' | '|' => return None,
                    c => {
                        self.at += 1;
                        c
                    }
                };
                if high < low {
                    return None;
                }
                items.push(ClassItem::Range(low, high));
            } else {
                items.push(ClassItem::Range(low, low));
            }
        }
        Some(Node::Class { negated, items })
    }

    /// An escape after its `\`, in a class or not.
    fn escape(&mut self, in_class: bool) -> Option<Escaped> {
        let c = self.peek()?;
        self.at += 1;
        Some(match c {
            'd' | 'D' => Escaped::Item(ClassItem::Digit(c == 'D')),
            'w' | 'W' => Escaped::Item(ClassItem::Word(c == 'W')),
            's' | 'S' => Escaped::Item(ClassItem::Space(c == 'S')),
            'n' => Escaped::Char('\n'),
            't' => Escaped::Char('\t'),
            'r' => Escaped::Char('\r'),
            'f' => Escaped::Char('\u{c}'),
            'v' => Escaped::Char('\u{b}'),
            '0' if !self.peek().is_some_and(|c| c.is_ascii_digit()) => Escaped::Char('\0'),
            'x' => Escaped::Char(self.hex(2)?),
            'u' if self.eat('{') => {
                let start = self.at;
                while self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                    self.at += 1;
                }
                let digits: String = self.chars[start..self.at].iter().collect();
                if !self.eat('}') {
                    return None;
                }
                Escaped::Char(char::from_u32(u32::from_str_radix(&digits, 16).ok()?)?)
            }
            'u' => Escaped::Char(self.hex(4)?),
            '^' | '$' | '\\' | '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '/' => {
                Escaped::Char(c)
            }
            // The `v` flag lets a class escape its punctuators too.
            '-' | '&' | '!' | '#' | '%' | ',' | ':' | ';' | '<' | '=' | '>' | '@' | '`' | '~' if in_class => {
                Escaped::Char(c)
            }
            _ => return None,
        })
    }

    fn hex(&mut self, count: usize) -> Option<char> {
        let digits: String = self.chars.get(self.at..self.at + count)?.iter().collect();
        if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        self.at += count;
        char::from_u32(u32::from_str_radix(&digits, 16).ok()?)
    }
}

enum Escaped {
    Char(char),
    Item(ClassItem),
}

/// A backtracking matcher: each step calls its continuation with where it ended.
struct Matcher<'a> {
    text: &'a [char],
    steps: usize,
}

type Continuation<'c> = dyn FnMut(usize, &mut Matcher<'_>) -> bool + 'c;

impl Matcher<'_> {
    fn alternatives(
        &mut self,
        alternatives: &Alternatives,
        at: usize,
        then: &mut dyn FnMut(usize, &mut Matcher<'_>) -> bool,
    ) -> bool {
        alternatives
            .iter()
            .any(|sequence| self.sequence(sequence, at, then))
    }

    fn sequence(&mut self, nodes: &[Node], at: usize, then: &mut Continuation<'_>) -> bool {
        self.steps += 1;
        if self.steps > BUDGET {
            return false;
        }
        let Some((first, rest)) = nodes.split_first() else {
            return then(at, self);
        };
        self.node(first, at, &mut |end, matcher: &mut Matcher<'_>| {
            matcher.sequence(rest, end, then)
        })
    }

    fn node(&mut self, node: &Node, at: usize, then: &mut Continuation<'_>) -> bool {
        let next = self.text.get(at).copied();
        match node {
            Node::Char(c) => next == Some(*c) && then(at + 1, self),
            Node::Any => next.is_some_and(|c| !is_line_terminator(c)) && then(at + 1, self),
            Node::Class { negated, items } => {
                next.is_some_and(|c| items.iter().any(|item| item.matches(c)) != *negated)
                    && then(at + 1, self)
            }
            Node::Group(alternatives) => self.alternatives(alternatives, at, then),
            Node::Look {
                negated,
                alternatives,
            } => {
                let found = self.alternatives(alternatives, at, &mut |_, _| true);
                found != *negated && then(at, self)
            }
            Node::Repeat { node, min, max } => self.repeat(node, (*min, *max), at, then),
            Node::Start => at == 0 && then(at, self),
            Node::End => at == self.text.len() && then(at, self),
        }
    }

    /// `node` at least `min` and at most `max` times from `at`, as often as it can first.
    fn repeat(
        &mut self,
        node: &Node,
        (min, max): (u32, Option<u32>),
        at: usize,
        then: &mut Continuation<'_>,
    ) -> bool {
        self.steps += 1;
        if self.steps > BUDGET {
            return false;
        }
        if max != Some(0)
            && self.node(node, at, &mut |end, matcher: &mut Matcher<'_>| {
                // An empty match repeats forever: it counts once.
                end != at && matcher.repeat(node, (min.saturating_sub(1), max.map(|max| max - 1)), end, then)
            })
        {
            return true;
        }
        if min == 0 {
            return then(at, self);
        }
        // An item that matches nothing may still meet its minimum.
        min > 0
            && self.node(node, at, &mut |end, matcher: &mut Matcher<'_>| {
                end == at && then(at, matcher)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::Pattern;

    fn whole(pattern: &str, text: &str) -> Option<bool> {
        Pattern::parse(pattern).and_then(|pattern| pattern.matches_whole(text))
    }

    #[test]
    fn matches_values_as_a_whole() {
        assert_eq!(whole("[A-Z]{4}", "ABCD"), Some(true));
        assert_eq!(whole("[A-Z]{4}", "ab12"), Some(false));
        assert_eq!(whole("[A-Z]{4}", "ABCDE"), Some(false));
        assert_eq!(whole(r"\d{5}", "79098"), Some(true));
        assert_eq!(whole(r"\d{4,5}", "123"), Some(false));
        assert_eq!(whole(r"[0-9]{3}-[0-9]{4}", "555-0199"), Some(true));
        assert_eq!(whole("cat|dog", "dog"), Some(true));
        assert_eq!(whole("cat|dog", "catdog"), Some(false));
        assert_eq!(whole(r"(\+49|0)[1-9]\d+", "+4976112345"), Some(true));
        assert_eq!(whole(r"(?=.*\d)(?=.*[a-z]).{8,}", "password1"), Some(true));
        assert_eq!(whole(r"(?=.*\d)(?=.*[a-z]).{8,}", "password"), Some(false));
        assert_eq!(whole("a*?b", "aaab"), Some(true));
        assert_eq!(whole(r"(a|ab)(c|bcd)(d*)", "abcd"), Some(true));
        assert_eq!(whole("[^@]+@[^@]+", "a@b"), Some(true));
        assert_eq!(whole(r"\w+\s\w+", "Ada Lovelace"), Some(true));
    }

    #[test]
    fn rejects_what_the_v_flag_rejects() {
        // An unescaped `-` at the end of a class, a lone quantifier, an unknown escape.
        assert!(Pattern::parse("[A-Za-z0-9-]+").is_none());
        assert!(Pattern::parse("*a").is_none());
        assert!(Pattern::parse(r"\a").is_none());
        assert!(Pattern::parse("(ab").is_none());
        assert!(Pattern::parse(r"[A-Za-z0-9\-]+").is_some());
    }

    #[test]
    fn gives_up_on_catastrophic_patterns() {
        let text = format!("{}b", "a".repeat(40));
        assert_eq!(whole("(a*)*c", &text), None);
    }
}

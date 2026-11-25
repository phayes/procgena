//! The "shown if" language: small conditions over other settings.

use serde_json::Value;

use super::path::{SettingPath, Template};

/// A condition over the settings in a tree, as written in a metadata
/// file's `shown_if` and `conditions`.
///
/// The language:
///
/// - `path`: the setting is on: `true`, a number other than zero, text
///   that is not empty, or any list or map. Missing settings and `null`
///   are off.
/// - `path == value`, `path != value`: the setting equals a number,
///   `true`, `false`, `null` or text (bare like `deciduous`, or quoted).
/// - `path > n`, `>=`, `<`, `<=`: numeric comparisons; false when the
///   setting is not a number.
/// - `path in [a, b]`: the setting equals one of the values.
/// - `!c`, `c && c`, `c || c` and parentheses. `&&` binds tighter than
///   `||`.
/// - `@name`: the named condition `name` from the file's `conditions`.
/// - `{index}` and `{parent}` (the index less one) anywhere in a path or
///   name, filled with the index the setting's `*` matched, such as
///   `branch.children.{parent} > 0` or `@grows_{index}`.
#[derive(Clone, Debug, PartialEq)]
pub struct Condition(Node);

#[derive(Clone, Debug, PartialEq)]
enum Node {
    On(Template),
    Compare(Template, Op, Literal),
    In(Template, Vec<Literal>),
    Named(Template),
    Not(Box<Node>),
    And(Box<Node>, Box<Node>),
    Or(Box<Node>, Box<Node>),
}

/// A value to compare a setting with.
#[derive(Clone, Debug, PartialEq)]
enum Literal {
    Value(Value),
    /// A number with `{index}` or `{parent}` in it, such as `{index}`.
    Indexed(Template),
}

impl Literal {
    /// The value, with the index filled in; `None` when it needs an index
    /// and there is none.
    fn value(&self, index: Option<usize>) -> Option<Value> {
        match self {
            Self::Value(value) => Some(value.clone()),
            Self::Indexed(template) => template.fill(index)?.parse::<i64>().ok().map(Value::from),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}

/// How deep named conditions may refer to each other before evaluation
/// gives up, which stops a condition that refers to itself.
const MAX_DEPTH: usize = 32;

/// Finds the named conditions a [`Condition`] refers to with `@name`.
pub trait NamedConditions {
    /// The condition named `name`, if there is one.
    fn named(&self, name: &str) -> Option<&Condition>;
}

impl Condition {
    /// Reads a condition.
    ///
    /// # Errors
    ///
    /// Returns a message naming what could not be read, such as a missing
    /// `)` or an unknown operator.
    pub fn parse(text: &str) -> Result<Self, String> {
        let tokens = tokenize(text)?;
        let mut parser = Parser { tokens, at: 0 };
        let node = parser.or()?;
        match parser.tokens.get(parser.at) {
            None => Ok(Self(node)),
            Some(token) => Err(format!("unexpected {token:?} in {text:?}")),
        }
    }

    /// A condition that holds when both `self` and `other` hold.
    #[must_use]
    pub fn and(self, other: Self) -> Self {
        Self(Node::And(Box::new(self.0), Box::new(other.0)))
    }

    /// Whether the condition holds for `value`, with `{index}` filled from
    /// `index` and `@name` looked up in `named`.
    ///
    /// A path that needs an index when there is none counts as a missing
    /// setting, as does an unknown name.
    #[must_use]
    pub fn holds(&self, value: &Value, index: Option<usize>, named: &dyn NamedConditions) -> bool {
        eval(&self.0, value, index, named, 0)
    }

    /// Every setting path the condition reads, with the index filled in.
    /// Paths that need an index when there is none are left out.
    #[must_use]
    pub fn paths(&self, index: Option<usize>) -> Vec<SettingPath> {
        let mut found = Vec::new();
        visit(&self.0, &mut |node| match node {
            Node::On(path) | Node::Compare(path, ..) | Node::In(path, _) => {
                found.extend(path.path(index));
            }
            _ => {}
        });
        found
    }

    /// Every name the condition refers to with `@name`, with the index
    /// filled in. Names that need an index when there is none are left out.
    #[must_use]
    pub fn names(&self, index: Option<usize>) -> Vec<String> {
        let mut found = Vec::new();
        visit(&self.0, &mut |node| {
            if let Node::Named(name) = node {
                found.extend(name.fill(index));
            }
        });
        found
    }

    /// Whether any path or name in the condition uses `{index}` or
    /// `{parent}`.
    #[must_use]
    pub fn uses_index(&self) -> bool {
        let mut uses = false;
        let indexed = |literal: &Literal| matches!(literal, Literal::Indexed(_));
        visit(&self.0, &mut |node| match node {
            Node::On(t) | Node::Named(t) => uses |= t.uses_index(),
            Node::Compare(t, _, literal) => uses |= t.uses_index() || indexed(literal),
            Node::In(t, literals) => uses |= t.uses_index() || literals.iter().any(indexed),
            _ => {}
        });
        uses
    }
}

fn visit(node: &Node, f: &mut impl FnMut(&Node)) {
    f(node);
    match node {
        Node::Not(inner) => visit(inner, f),
        Node::And(a, b) | Node::Or(a, b) => {
            visit(a, f);
            visit(b, f);
        }
        _ => {}
    }
}

fn eval(
    node: &Node,
    value: &Value,
    index: Option<usize>,
    named: &dyn NamedConditions,
    depth: usize,
) -> bool {
    let read = |path: &Template| path.path(index).and_then(|path| path.get(value).cloned());
    match node {
        Node::On(path) => read(path).is_some_and(|v| is_on(&v)),
        Node::Compare(path, op, wanted) => read(path)
            .zip(wanted.value(index))
            .is_some_and(|(v, wanted)| compare(&v, *op, &wanted)),
        Node::In(path, options) => read(path).is_some_and(|v| {
            options
                .iter()
                .filter_map(|o| o.value(index))
                .any(|o| compare(&v, Op::Eq, &o))
        }),
        Node::Named(name) => {
            depth < MAX_DEPTH
                && name
                    .fill(index)
                    .and_then(|name| named.named(&name))
                    .is_some_and(|c| eval(&c.0, value, index, named, depth + 1))
        }
        Node::Not(inner) => !eval(inner, value, index, named, depth),
        Node::And(a, b) => {
            eval(a, value, index, named, depth) && eval(b, value, index, named, depth)
        }
        Node::Or(a, b) => {
            eval(a, value, index, named, depth) || eval(b, value, index, named, depth)
        }
    }
}

/// Whether a setting counts as on.
fn is_on(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(on) => *on,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

fn compare(value: &Value, op: Op, wanted: &Value) -> bool {
    match (value.as_f64(), wanted.as_f64()) {
        (Some(a), Some(b)) => match op {
            Op::Eq => a == b,
            Op::Ne => a != b,
            Op::Gt => a > b,
            Op::Ge => a >= b,
            Op::Lt => a < b,
            Op::Le => a <= b,
        },
        _ => match op {
            Op::Eq => value == wanted,
            Op::Ne => value != wanted,
            _ => false,
        },
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Word(String),
    Quoted(String),
    Op(&'static str),
}

const OPERATORS: [&str; 13] = [
    "&&", "||", "==", "!=", ">=", "<=", ">", "<", "!", "(", ")", "[", "]",
];

fn tokenize(text: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut rest = text.trim_start();
    while !rest.is_empty() {
        if let Some(op) = OPERATORS.iter().find(|op| rest.starts_with(**op)) {
            tokens.push(Token::Op(op));
            rest = &rest[op.len()..];
        } else if rest.starts_with(',') {
            tokens.push(Token::Op(","));
            rest = &rest[1..];
        } else if let Some(quoted) = rest.strip_prefix('"') {
            let end = quoted
                .find('"')
                .ok_or_else(|| format!("unclosed quote in {text:?}"))?;
            tokens.push(Token::Quoted(quoted[..end].to_owned()));
            rest = &quoted[end + 1..];
        } else {
            let end = rest
                .find(|c: char| c.is_whitespace() || "&|=!<>()[],\"".contains(c))
                .unwrap_or(rest.len());
            if end == 0 {
                return Err(format!("unexpected {:?} in {text:?}", &rest[..1]));
            }
            tokens.push(Token::Word(rest[..end].to_owned()));
            rest = &rest[end..];
        }
        rest = rest.trim_start();
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

impl Parser {
    fn peek_op(&self, op: &str) -> bool {
        matches!(self.tokens.get(self.at), Some(Token::Op(o)) if *o == op)
    }

    fn take_op(&mut self, op: &str) -> bool {
        let found = self.peek_op(op);
        self.at += usize::from(found);
        found
    }

    fn next(&mut self) -> Result<Token, String> {
        let token = self
            .tokens
            .get(self.at)
            .cloned()
            .ok_or("the condition ends too early")?;
        self.at += 1;
        Ok(token)
    }

    fn or(&mut self) -> Result<Node, String> {
        let mut node = self.and()?;
        while self.take_op("||") {
            node = Node::Or(Box::new(node), Box::new(self.and()?));
        }
        Ok(node)
    }

    fn and(&mut self) -> Result<Node, String> {
        let mut node = self.unary()?;
        while self.take_op("&&") {
            node = Node::And(Box::new(node), Box::new(self.unary()?));
        }
        Ok(node)
    }

    fn unary(&mut self) -> Result<Node, String> {
        if self.take_op("!") {
            return Ok(Node::Not(Box::new(self.unary()?)));
        }
        if self.take_op("(") {
            let node = self.or()?;
            if !self.take_op(")") {
                return Err("missing )".to_owned());
            }
            return Ok(node);
        }
        let Token::Word(word) = self.next()? else {
            return Err("expected a setting path".to_owned());
        };
        if let Some(name) = word.strip_prefix('@') {
            return Ok(Node::Named(Template::parse(name)?));
        }
        let path = Template::parse(&word)?;
        let op = ["==", "!=", ">=", "<=", ">", "<"]
            .into_iter()
            .find(|op| self.peek_op(op));
        if let Some(op) = op {
            self.at += 1;
            let op = match op {
                "==" => Op::Eq,
                "!=" => Op::Ne,
                ">=" => Op::Ge,
                "<=" => Op::Le,
                ">" => Op::Gt,
                _ => Op::Lt,
            };
            return Ok(Node::Compare(path, op, self.literal()?));
        }
        if matches!(self.tokens.get(self.at), Some(Token::Word(w)) if w == "in") {
            self.at += 1;
            if !self.take_op("[") {
                return Err("expected [ after in".to_owned());
            }
            let mut options = Vec::new();
            while !self.take_op("]") {
                options.push(self.literal()?);
                if !self.take_op(",") && !self.peek_op("]") {
                    return Err("expected , or ] in a list".to_owned());
                }
            }
            return Ok(Node::In(path, options));
        }
        Ok(Node::On(path))
    }

    fn literal(&mut self) -> Result<Literal, String> {
        Ok(Literal::Value(match self.next()? {
            Token::Quoted(text) => Value::String(text),
            Token::Word(word) if word.contains('{') => {
                return Ok(Literal::Indexed(Template::parse(&word)?));
            }
            Token::Word(word) => match word.as_str() {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                "null" => Value::Null,
                _ => word
                    .parse::<i64>()
                    .map(Value::from)
                    .or_else(|_| word.parse::<f64>().map(Value::from))
                    .unwrap_or(Value::String(word)),
            },
            Token::Op(op) => return Err(format!("expected a value, found {op:?}")),
        }))
    }
}

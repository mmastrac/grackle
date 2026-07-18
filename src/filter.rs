//! The filter language for views (DESIGN.md §5).
//!
//! Deliberately tiny: a predicate over row fields, not SQL. Anything fancier
//! should be a named Rust generator instead of growing this.
//!
//! ```text
//! !draft && !hidden
//! hidden || draft
//! year >= 2020 && "rust" in tags
//! layout == "post" && !(draft || hidden)
//! *
//! ```
//!
//! Expressions are parsed and **type-checked against a schema at load time**,
//! so a typo (`!drafts`) is a startup error naming the view — not a filter that
//! silently matches everything.

use anyhow::{anyhow, bail, Result};
use std::collections::BTreeMap;
use std::fmt;

// ------------------------------------------------------------------ types

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    Bool,
    Int,
    /// Includes dates, which are ISO-8601 and so order correctly as strings.
    Str,
    List,
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Type::Bool => "bool",
            Type::Int => "int",
            Type::Str => "string",
            Type::List => "list",
        })
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(untagged)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Str(String),
    List(Vec<String>),
    Null,
}

impl Value {
    /// Truthiness for a bare field reference: `description` means "has one".
    fn truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Str(s) => !s.is_empty(),
            Value::List(v) => !v.is_empty(),
            Value::Null => false,
        }
    }
}

pub type Schema = BTreeMap<&'static str, Type>;

/// A row the filter can read fields from.
pub trait Row {
    fn field(&self, name: &str) -> Value;
}

// ------------------------------------------------------------------ ast

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl Op {
    fn is_ordering(self) -> bool {
        !matches!(self, Op::Eq | Op::Ne)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Lit {
    Int(i64),
    Str(String),
    Bool(bool),
}

impl Lit {
    fn ty(&self) -> Type {
        match self {
            Lit::Int(_) => Type::Int,
            Lit::Str(_) => Type::Str,
            Lit::Bool(_) => Type::Bool,
        }
    }
    fn value(&self) -> Value {
        match self {
            Lit::Int(i) => Value::Int(*i),
            Lit::Str(s) => Value::Str(s.clone()),
            Lit::Bool(b) => Value::Bool(*b),
        }
    }
}

#[derive(Debug, Clone)]
enum Expr {
    True,
    Truthy(String),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Cmp(String, Op, Lit),
    /// `"rust" in tags`
    In(Lit, String),
}

// ------------------------------------------------------------------ lexer

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Int(i64),
    Str(String),
    Star,
    Bang,
    And,
    Or,
    Cmp(Op),
    In,
    LParen,
    RParen,
}

fn lex(src: &str) -> Result<Vec<Tok>> {
    let b: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i];
        match c {
            c if c.is_whitespace() => i += 1,
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            '*' => {
                out.push(Tok::Star);
                i += 1;
            }
            '!' => {
                if b.get(i + 1) == Some(&'=') {
                    out.push(Tok::Cmp(Op::Ne));
                    i += 2;
                } else {
                    out.push(Tok::Bang);
                    i += 1;
                }
            }
            '=' => {
                if b.get(i + 1) == Some(&'=') {
                    out.push(Tok::Cmp(Op::Eq));
                    i += 2;
                } else {
                    bail!("expected `==` at offset {i}, found a single `=`");
                }
            }
            '<' | '>' => {
                let eq = b.get(i + 1) == Some(&'=');
                let op = match (c, eq) {
                    ('<', false) => Op::Lt,
                    ('<', true) => Op::Le,
                    ('>', false) => Op::Gt,
                    _ => Op::Ge,
                };
                out.push(Tok::Cmp(op));
                i += if eq { 2 } else { 1 };
            }
            '&' => {
                if b.get(i + 1) == Some(&'&') {
                    out.push(Tok::And);
                    i += 2;
                } else {
                    bail!("expected `&&` at offset {i}");
                }
            }
            '|' => {
                if b.get(i + 1) == Some(&'|') {
                    out.push(Tok::Or);
                    i += 2;
                } else {
                    bail!("expected `||` at offset {i}");
                }
            }
            '"' | '\'' => {
                let quote = c;
                i += 1;
                let start = i;
                while i < b.len() && b[i] != quote {
                    i += 1;
                }
                if i >= b.len() {
                    bail!("unterminated string starting at offset {}", start - 1);
                }
                out.push(Tok::Str(b[start..i].iter().collect()));
                i += 1;
            }
            c if c.is_ascii_digit() || (c == '-' && b.get(i + 1).is_some_and(|d| d.is_ascii_digit())) => {
                let start = i;
                if c == '-' {
                    i += 1;
                }
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
                let s: String = b[start..i].iter().collect();
                out.push(Tok::Int(s.parse()?));
            }
            c if c.is_alphabetic() || c == '_' => {
                let start = i;
                while i < b.len() && (b[i].is_alphanumeric() || b[i] == '_' || b[i] == '.') {
                    i += 1;
                }
                let s: String = b[start..i].iter().collect();
                out.push(match s.as_str() {
                    "in" => Tok::In,
                    "true" => Tok::Ident("true".into()),
                    "false" => Tok::Ident("false".into()),
                    _ => Tok::Ident(s),
                });
            }
            _ => bail!("unexpected character {c:?} at offset {i}"),
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------ parser

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }
    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == Some(t) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn parse_or(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_and()?;
        while self.eat(&Tok::Or) {
            let rhs = self.parse_and()?;
            lhs = Expr::Or(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr> {
        let mut lhs = self.parse_unary()?;
        while self.eat(&Tok::And) {
            let rhs = self.parse_unary()?;
            lhs = Expr::And(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr> {
        if self.eat(&Tok::Bang) {
            return Ok(Expr::Not(Box::new(self.parse_unary()?)));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        match self.next() {
            Some(Tok::LParen) => {
                let e = self.parse_or()?;
                if !self.eat(&Tok::RParen) {
                    bail!("expected `)`");
                }
                Ok(e)
            }
            Some(Tok::Star) => Ok(Expr::True),
            Some(Tok::Ident(name)) => {
                if name == "true" {
                    return Ok(Expr::True);
                }
                if name == "false" {
                    return Ok(Expr::Not(Box::new(Expr::True)));
                }
                match self.peek().cloned() {
                    Some(Tok::Cmp(op)) => {
                        self.pos += 1;
                        let lit = self.parse_lit()?;
                        Ok(Expr::Cmp(name, op, lit))
                    }
                    _ => Ok(Expr::Truthy(name)),
                }
            }
            // `"rust" in tags`
            Some(t @ (Tok::Str(_) | Tok::Int(_))) => {
                let lit = match t {
                    Tok::Str(s) => Lit::Str(s),
                    Tok::Int(i) => Lit::Int(i),
                    _ => unreachable!(),
                };
                if !self.eat(&Tok::In) {
                    bail!("a literal is only valid on the left of `in` (as in `\"rust\" in tags`)");
                }
                match self.next() {
                    Some(Tok::Ident(field)) => Ok(Expr::In(lit, field)),
                    _ => bail!("expected a field name after `in`"),
                }
            }
            Some(t) => bail!("unexpected token {t:?}"),
            None => bail!("unexpected end of expression"),
        }
    }

    fn parse_lit(&mut self) -> Result<Lit> {
        match self.next() {
            Some(Tok::Str(s)) => Ok(Lit::Str(s)),
            Some(Tok::Int(i)) => Ok(Lit::Int(i)),
            Some(Tok::Ident(i)) if i == "true" => Ok(Lit::Bool(true)),
            Some(Tok::Ident(i)) if i == "false" => Ok(Lit::Bool(false)),
            Some(t) => bail!("expected a literal, found {t:?}"),
            None => bail!("expected a literal"),
        }
    }
}

// ------------------------------------------------------------ validation

fn levenshtein(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

fn resolve(schema: &Schema, name: &str) -> Result<Type> {
    if let Some(t) = schema.get(name) {
        return Ok(*t);
    }
    let mut known: Vec<&str> = schema.keys().copied().collect();
    known.sort_unstable();
    let hint = known
        .iter()
        .map(|k| (levenshtein(name, k), *k))
        .filter(|(d, _)| *d <= 2)
        .min_by_key(|(d, _)| *d)
        .map(|(_, k)| format!(" (did you mean `{k}`?)"))
        .unwrap_or_default();
    Err(anyhow!(
        "unknown field `{name}`{hint}\n  known fields: {}",
        known.join(", ")
    ))
}

fn check(e: &Expr, schema: &Schema) -> Result<()> {
    match e {
        Expr::True => Ok(()),
        Expr::Truthy(f) => resolve(schema, f).map(|_| ()),
        Expr::Not(x) => check(x, schema),
        Expr::And(a, b) | Expr::Or(a, b) => {
            check(a, schema)?;
            check(b, schema)
        }
        Expr::Cmp(f, op, lit) => {
            let ft = resolve(schema, f)?;
            if ft == Type::List {
                bail!(
                    "`{f}` is a list; use `{} in {f}` instead of a comparison",
                    match lit {
                        Lit::Str(s) => format!("{s:?}"),
                        other => format!("{other:?}"),
                    }
                );
            }
            let lt = lit.ty();
            if ft != lt {
                bail!("`{f}` is {ft}, but it is compared to a {lt} literal");
            }
            if op.is_ordering() && ft == Type::Bool {
                bail!("`{f}` is bool; ordering comparisons are not meaningful");
            }
            Ok(())
        }
        Expr::In(lit, f) => {
            let ft = resolve(schema, f)?;
            if ft != Type::List {
                bail!("`in` needs a list on the right, but `{f}` is {ft}");
            }
            if lit.ty() != Type::Str {
                bail!("`in` needs a string on the left");
            }
            Ok(())
        }
    }
}

// ------------------------------------------------------------------ api

#[derive(Debug, Clone)]
pub struct Filter {
    ast: Expr,
}

impl Filter {
    /// Parse and type-check against `schema`.
    pub fn parse(src: &str, schema: &Schema) -> Result<Self> {
        let toks = lex(src)?;
        if toks.is_empty() {
            return Ok(Filter { ast: Expr::True });
        }
        let mut p = Parser { toks, pos: 0 };
        let ast = p.parse_or()?;
        if p.pos != p.toks.len() {
            bail!("trailing tokens after a complete expression");
        }
        check(&ast, schema)?;
        Ok(Filter { ast })
    }

    /// Always-true filter, for an absent `filter =`.
    pub fn always() -> Self {
        Filter { ast: Expr::True }
    }

    pub fn eval(&self, row: &impl Row) -> bool {
        eval(&self.ast, row)
    }
}

fn cmp_values(a: &Value, op: Op, b: &Value) -> bool {
    use std::cmp::Ordering;
    let ord = match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.partial_cmp(y),
        (Value::Str(x), Value::Str(y)) => x.partial_cmp(y),
        (Value::Bool(x), Value::Bool(y)) => x.partial_cmp(y),
        // A null field compares equal to nothing and orders below everything.
        (Value::Null, _) => return matches!(op, Op::Ne),
        _ => None,
    };
    match ord {
        Some(o) => match op {
            Op::Eq => o == Ordering::Equal,
            Op::Ne => o != Ordering::Equal,
            Op::Lt => o == Ordering::Less,
            Op::Le => o != Ordering::Greater,
            Op::Gt => o == Ordering::Greater,
            Op::Ge => o != Ordering::Less,
        },
        None => false,
    }
}

fn eval(e: &Expr, row: &impl Row) -> bool {
    match e {
        Expr::True => true,
        Expr::Truthy(f) => row.field(f).truthy(),
        Expr::Not(x) => !eval(x, row),
        Expr::And(a, b) => eval(a, row) && eval(b, row),
        Expr::Or(a, b) => eval(a, row) || eval(b, row),
        Expr::Cmp(f, op, lit) => cmp_values(&row.field(f), *op, &lit.value()),
        Expr::In(lit, f) => match (row.field(f), lit) {
            (Value::List(items), Lit::Str(s)) => items.iter().any(|i| i == s),
            _ => false,
        },
    }
}

// ------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;

    struct TestRow {
        draft: bool,
        hidden: bool,
        year: i64,
        title: String,
        description: Option<String>,
        tags: Vec<String>,
    }

    impl Default for TestRow {
        fn default() -> Self {
            TestRow {
                draft: false,
                hidden: false,
                year: 2022,
                title: "Hello".into(),
                description: None,
                tags: vec!["rust".into(), "c".into()],
            }
        }
    }

    impl Row for TestRow {
        fn field(&self, name: &str) -> Value {
            match name {
                "draft" => Value::Bool(self.draft),
                "hidden" => Value::Bool(self.hidden),
                "year" => Value::Int(self.year),
                "title" => Value::Str(self.title.clone()),
                "description" => match &self.description {
                    Some(d) => Value::Str(d.clone()),
                    None => Value::Null,
                },
                "tags" => Value::List(self.tags.clone()),
                _ => Value::Null,
            }
        }
    }

    fn schema() -> Schema {
        let mut s = Schema::new();
        s.insert("draft", Type::Bool);
        s.insert("hidden", Type::Bool);
        s.insert("year", Type::Int);
        s.insert("title", Type::Str);
        s.insert("description", Type::Str);
        s.insert("tags", Type::List);
        s
    }

    fn ok(src: &str) -> Filter {
        Filter::parse(src, &schema()).expect(src)
    }

    #[test]
    fn the_configs_own_filters() {
        let r = TestRow::default();
        assert!(ok("!draft").eval(&r));
        assert!(ok("!draft && !hidden").eval(&r));
        assert!(ok("*").eval(&r));
        assert!(!ok("hidden").eval(&r));
    }

    #[test]
    fn boolean_algebra() {
        let r = TestRow {
            draft: true,
            ..Default::default()
        };
        assert!(!ok("!draft").eval(&r));
        assert!(ok("draft || hidden").eval(&r));
        assert!(!ok("draft && hidden").eval(&r));
        assert!(ok("!(draft && hidden)").eval(&r));
        assert!(!ok("!(draft || hidden)").eval(&r));
    }

    #[test]
    fn precedence_and_binds_tighter_than_or() {
        // hidden || (draft && hidden) == false for this row
        let r = TestRow {
            draft: true,
            hidden: false,
            ..Default::default()
        };
        assert!(!ok("hidden || draft && hidden").eval(&r));
        assert!(ok("(hidden || draft) && !hidden").eval(&r));
    }

    #[test]
    fn comparisons_and_membership() {
        let r = TestRow::default();
        assert!(ok("year >= 2020").eval(&r));
        assert!(ok("year == 2022").eval(&r));
        assert!(!ok("year < 2000").eval(&r));
        assert!(ok("title == \"Hello\"").eval(&r));
        assert!(ok("\"rust\" in tags").eval(&r));
        assert!(!ok("\"python\" in tags").eval(&r));
        assert!(ok("year >= 2020 && \"rust\" in tags").eval(&r));
    }

    #[test]
    fn truthiness_of_nullable_and_lists() {
        let r = TestRow::default();
        assert!(!ok("description").eval(&r), "null description is falsy");
        assert!(ok("!description").eval(&r));
        assert!(ok("tags").eval(&r), "non-empty list is truthy");
        let r2 = TestRow {
            tags: vec![],
            description: Some("x".into()),
            ..Default::default()
        };
        assert!(!ok("tags").eval(&r2));
        assert!(ok("description").eval(&r2));
    }

    /// The whole point: a typo must not silently match everything.
    #[test]
    fn unknown_field_is_an_error_with_a_hint() {
        let e = Filter::parse("!drafts", &schema()).unwrap_err().to_string();
        assert!(e.contains("unknown field `drafts`"), "{e}");
        assert!(e.contains("did you mean `draft`"), "{e}");
        assert!(e.contains("known fields:"), "{e}");
    }

    #[test]
    fn type_errors_are_caught_at_parse_time() {
        let e = |s: &str| Filter::parse(s, &schema()).unwrap_err().to_string();
        assert!(e("year == \"2022\"").contains("compared to a string literal"));
        assert!(e("title > 5").contains("compared to a int literal"));
        assert!(e("tags == \"rust\"").contains("use `\"rust\" in tags`"));
        assert!(e("\"x\" in title").contains("`in` needs a list"));
        assert!(e("draft > 1").contains("bool"));
    }

    #[test]
    fn syntax_errors() {
        let e = |s: &str| Filter::parse(s, &schema()).unwrap_err().to_string();
        assert!(e("draft &").contains("expected `&&`"));
        assert!(e("(draft").contains("expected `)`"));
        assert!(e("draft = true").contains("expected `==`"));
        assert!(e("\"rust\"").contains("`in`"));
        assert!(e("draft draft").contains("trailing tokens"));
    }

    #[test]
    fn empty_filter_matches_everything() {
        let r = TestRow::default();
        assert!(Filter::parse("", &schema()).unwrap().eval(&r));
        assert!(Filter::always().eval(&r));
    }
}

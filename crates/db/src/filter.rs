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
//! under(path, "recipes") && !draft
//! *
//! ```
//!
//! Expressions are parsed and **type-checked against a schema at load time**,
//! so a typo (`!drafts`) is a startup error naming the view — not a filter that
//! silently matches everything.
//!
//! Functions are the extension point. A field, a literal and a call are one
//! thing — an operand — so a call goes anywhere a field does, nests, and is
//! type-checked by the same pass. Adding one is an entry in `FUNCS`.

use anyhow::{anyhow, bail, Result};
use std::collections::BTreeMap;
use std::fmt;

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
    /// As an error message names it.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Bool(_) => "a bool",
            Value::Int(_) => "an int",
            Value::Str(_) => "a string",
            Value::List(_) => "a list",
            Value::Null => "null",
        }
    }

    /// A total order over values, for sorting.
    ///
    /// Distinct from what comparison in a FILTER does, deliberately. There,
    /// `Null` compares equal to nothing — a row with no date matches neither
    /// `date > x` nor `date < x`. Here it has to land somewhere, and it lands
    /// last: an undated row sorts after every dated one.
    ///
    /// Last regardless of direction, which is why reversing is not simply
    /// `Ordering::reverse` on the result. Descending by date means the newest
    /// first and the undated still at the end, not the undated first.
    pub fn order(&self, other: &Value, desc: bool) -> std::cmp::Ordering {
        use std::cmp::Ordering::*;
        let ord = match (self, other) {
            (Value::Null, Value::Null) => return Equal,
            (Value::Null, _) => return Greater,
            (_, Value::Null) => return Less,
            (Value::Str(x), Value::Str(y)) => x.cmp(y),
            (Value::Int(x), Value::Int(y)) => x.cmp(y),
            (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
            (Value::List(x), Value::List(y)) => x.cmp(y),
            // Mixed types cannot arise: a column has one type in the schema.
            _ => Equal,
        };
        if desc {
            ord.reverse()
        } else {
            ord
        }
    }

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

/// Something that produces a value: a field, a literal, or a call.
///
/// Calls nest, because there is no reason for `under(dir(path), "x")` to be a
/// special case — an operand is an operand.
#[derive(Debug, Clone)]
enum Operand {
    Field(String),
    Lit(Lit),
    Call(&'static Func, Vec<Operand>, Prepared),
}

#[derive(Debug, Clone)]
enum Expr {
    True,
    Truthy(Operand),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Cmp(Operand, Op, Lit),
    /// `"rust" in tags`
    In(Lit, Operand),
}

/// A function the language can call. The signature drives type checking and
/// `eval` drives evaluation, so adding one is a single entry here.
///
/// Deliberately a fixed table rather than a registry callers extend: the
/// function set is part of the language, and a filter that parses against one
/// caller's vocabulary and not another's is not a language.
pub struct Func {
    name: &'static str,
    params: &'static [Type],
    returns: Type,
    /// Whatever the function can work out once, at parse time, from its
    /// literal arguments. A glob is a regex; compiling one per row per filter
    /// is not a trade worth making, and a pattern that varies per row is not
    /// a thing anyone wants.
    prepare: fn(&[Operand]) -> Result<Prepared>,
    eval: fn(&Prepared, &[Value]) -> Value,
}

/// The closed set of things a function may precompute. Small on purpose: it
/// is a cache, not a second value type.
#[derive(Debug, Clone)]
enum Prepared {
    None,
    Glob(globset::GlobMatcher),
}

fn no_prep(_: &[Operand]) -> Result<Prepared> {
    Ok(Prepared::None)
}

/// The literal a function needs at parse time, or an error naming why.
fn literal_arg<'a>(args: &'a [Operand], i: usize, func: &str) -> Result<&'a str> {
    match args.get(i) {
        Some(Operand::Lit(Lit::Str(s))) => Ok(s),
        _ => bail!("`{func}` argument {} must be a literal string", i + 1),
    }
}

impl fmt::Debug for Func {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name)
    }
}

/// Is `p` at or below `base`, comparing whole path segments?
///
/// Segment-wise is the entire point. `recipes` must not claim `recipes-old`,
/// which a string prefix would, and which is why this is a function rather
/// than something spelled with `>=`. Reflexive: `recipes` is under `recipes`,
/// so a directory's own index page belongs to it. An empty base is the root
/// and holds everything.
fn path_under(p: &str, base: &str) -> bool {
    let base = base.trim_matches('/');
    if base.is_empty() {
        return true;
    }
    let mut parts = p.trim_matches('/').split('/');
    base.split('/').all(|b| parts.next() == Some(b))
}

const FUNCS: &[Func] = &[
    Func {
        name: "under",
        params: &[Type::Str, Type::Str],
        returns: Type::Bool,
        prepare: no_prep,
        eval: |_, args| match (&args[0], &args[1]) {
            (Value::Str(p), Value::Str(base)) => Value::Bool(path_under(p, base)),
            // A null path is under nothing, which is what a row with no path
            // should mean rather than an error at query time.
            _ => Value::Bool(false),
        },
    },
    Func {
        name: "glob",
        params: &[Type::Str, Type::Str],
        returns: Type::Bool,
        prepare: |args| {
            let pat = literal_arg(args, 1, "glob")?;
            Ok(Prepared::Glob(
                globset::Glob::new(pat)
                    .map_err(|e| anyhow!("bad glob {pat:?}: {e}"))?
                    .compile_matcher(),
            ))
        },
        eval: |prep, args| match (prep, &args[0]) {
            (Prepared::Glob(m), Value::Str(p)) => Value::Bool(m.is_match(p)),
            _ => Value::Bool(false),
        },
    },
];

fn lookup_func(name: &str) -> Result<&'static Func> {
    if let Some(f) = FUNCS.iter().find(|f| f.name == name) {
        return Ok(f);
    }
    let known: Vec<&str> = FUNCS.iter().map(|f| f.name).collect();
    let hint = known
        .iter()
        .map(|k| (levenshtein(name, k), *k))
        .filter(|(d, _)| *d <= 2)
        .min_by_key(|(d, _)| *d)
        .map(|(_, k)| format!(" (did you mean `{k}`?)"))
        .unwrap_or_default();
    Err(anyhow!(
        "unknown function `{name}`{hint}\n  known functions: {}",
        known.join(", ")
    ))
}

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
    Comma,
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
            ',' => {
                out.push(Tok::Comma);
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
            c if c.is_ascii_digit()
                || (c == '-' && b.get(i + 1).is_some_and(|d| d.is_ascii_digit())) =>
            {
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
                let operand = self.parse_call_or_field(name)?;
                match self.peek().cloned() {
                    Some(Tok::Cmp(op)) => {
                        self.pos += 1;
                        let lit = self.parse_lit()?;
                        Ok(Expr::Cmp(operand, op, lit))
                    }
                    _ => Ok(Expr::Truthy(operand)),
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
                    Some(Tok::Ident(name)) => Ok(Expr::In(lit, self.parse_call_or_field(name)?)),
                    _ => bail!("expected a field name after `in`"),
                }
            }
            Some(t) => bail!("unexpected token {t:?}"),
            None => bail!("unexpected end of expression"),
        }
    }

    /// An identifier is a call when a `(` follows it, and a field otherwise.
    fn parse_call_or_field(&mut self, name: String) -> Result<Operand> {
        if !self.eat(&Tok::LParen) {
            return Ok(Operand::Field(name));
        }
        let func = lookup_func(&name)?;
        let mut args = Vec::new();
        if !self.eat(&Tok::RParen) {
            loop {
                args.push(self.parse_operand()?);
                if self.eat(&Tok::RParen) {
                    break;
                }
                if !self.eat(&Tok::Comma) {
                    bail!("expected `,` or `)` in the arguments to `{name}`");
                }
            }
        }
        // Arity first: `prepare` reaches for an argument by position, and a
        // call with too few would otherwise report the wrong complaint.
        if args.len() != func.params.len() {
            bail!(
                "`{name}` takes {} argument{}, but {} were given",
                func.params.len(),
                if func.params.len() == 1 { "" } else { "s" },
                args.len()
            );
        }
        let prep = (func.prepare)(&args)?;
        Ok(Operand::Call(func, args, prep))
    }

    fn parse_operand(&mut self) -> Result<Operand> {
        match self.next() {
            Some(Tok::Ident(name)) if name != "true" && name != "false" => {
                self.parse_call_or_field(name)
            }
            Some(Tok::Ident(name)) => Ok(Operand::Lit(Lit::Bool(name == "true"))),
            Some(Tok::Str(s)) => Ok(Operand::Lit(Lit::Str(s))),
            Some(Tok::Int(i)) => Ok(Operand::Lit(Lit::Int(i))),
            Some(t) => bail!("expected a field, literal or call, found {t:?}"),
            None => bail!("expected an argument"),
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

impl fmt::Display for Operand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Operand::Field(name) => f.write_str(name),
            Operand::Lit(Lit::Str(s)) => write!(f, "{s:?}"),
            Operand::Lit(Lit::Int(i)) => write!(f, "{i}"),
            Operand::Lit(Lit::Bool(b)) => write!(f, "{b}"),
            Operand::Call(func, args, _) => {
                write!(f, "{}(", func.name)?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{a}")?;
                }
                f.write_str(")")
            }
        }
    }
}

/// The type an operand produces, checking a call's arity and argument types
/// on the way through.
fn operand_type(o: &Operand, schema: &Schema) -> Result<Type> {
    match o {
        Operand::Field(name) => resolve(schema, name),
        Operand::Lit(lit) => Ok(lit.ty()),
        // Arity was settled at parse time, so `zip` cannot silently skip an
        // argument here.
        Operand::Call(func, args, _) => {
            for (i, (arg, want)) in args.iter().zip(func.params).enumerate() {
                let got = operand_type(arg, schema)?;
                if got != *want {
                    bail!(
                        "`{}` argument {} is {want}, but `{arg}` is {got}",
                        func.name,
                        i + 1
                    );
                }
            }
            Ok(func.returns)
        }
    }
}

fn check(e: &Expr, schema: &Schema) -> Result<()> {
    match e {
        Expr::True => Ok(()),
        Expr::Truthy(o) => operand_type(o, schema).map(|_| ()),
        Expr::Not(x) => check(x, schema),
        Expr::And(a, b) | Expr::Or(a, b) => {
            check(a, schema)?;
            check(b, schema)
        }
        Expr::Cmp(f, op, lit) => {
            let ft = operand_type(f, schema)?;
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
            let ft = operand_type(f, schema)?;
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

#[derive(Debug, Clone)]
pub struct Filter {
    ast: Expr,
}

impl Filter {
    /// Both, or nothing. Lets a caller narrow a declared filter with one it
    /// derived, without either having to know the other's source text.
    pub fn and(self, other: Filter) -> Filter {
        Filter {
            ast: Expr::And(Box::new(self.ast), Box::new(other.ast)),
        }
    }

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

/// An operand's value for one row. Arity and types were settled at parse
/// time, so a call here is a lookup and an apply.
fn operand_value(o: &Operand, row: &impl Row) -> Value {
    match o {
        Operand::Field(name) => row.field(name),
        Operand::Lit(lit) => lit.value(),
        Operand::Call(func, args, prep) => {
            let vals: Vec<Value> = args.iter().map(|a| operand_value(a, row)).collect();
            (func.eval)(prep, &vals)
        }
    }
}

fn eval(e: &Expr, row: &impl Row) -> bool {
    match e {
        Expr::True => true,
        Expr::Truthy(o) => operand_value(o, row).truthy(),
        Expr::Not(x) => !eval(x, row),
        Expr::And(a, b) => eval(a, row) && eval(b, row),
        Expr::Or(a, b) => eval(a, row) || eval(b, row),
        Expr::Cmp(f, op, lit) => cmp_values(&operand_value(f, row), *op, &lit.value()),
        Expr::In(lit, f) => match (operand_value(f, row), lit) {
            (Value::List(items), Lit::Str(s)) => items.iter().any(|i| i == s),
            _ => false,
        },
    }
}

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
        path: String,
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
                path: "recipes/pasta/carbonara.md".into(),
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
                "path" => Value::Str(self.path.clone()),
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
        s.insert("path", Type::Str);
        s
    }

    fn ok(src: &str) -> Filter {
        Filter::parse(src, &schema()).expect(src)
    }

    fn err(src: &str) -> String {
        Filter::parse(src, &schema()).expect_err(src).to_string()
    }

    #[test]
    fn under_selects_a_subtree() {
        let r = TestRow::default(); // recipes/pasta/carbonara.md
        assert!(ok(r#"under(path, "recipes")"#).eval(&r));
        assert!(ok(r#"under(path, "recipes/pasta")"#).eval(&r));
        assert!(!ok(r#"under(path, "books")"#).eval(&r));
    }

    /// The reason this is a function and not a `starts_with` on strings: a
    /// sibling directory sharing a name prefix is not a child.
    #[test]
    fn under_compares_whole_segments() {
        let r = TestRow {
            path: "recipes-old/x.md".into(),
            ..Default::default()
        };
        assert!(!ok(r#"under(path, "recipes")"#).eval(&r));
    }

    #[test]
    fn under_is_reflexive_and_rooted() {
        let r = TestRow {
            path: "recipes".into(),
            ..Default::default()
        };
        assert!(
            ok(r#"under(path, "recipes")"#).eval(&r),
            "a dir is under itself"
        );
        assert!(
            ok(r#"under(path, "")"#).eval(&r),
            "everything is under the root"
        );
        assert!(
            ok(r#"under(path, "/recipes/")"#).eval(&r),
            "surrounding slashes are noise"
        );
    }

    #[test]
    fn glob_matches_patterns_the_config_already_writes() {
        let r = TestRow::default(); // recipes/pasta/carbonara.md
        assert!(ok(r#"glob(path, "recipes/**")"#).eval(&r));
        assert!(ok(r#"glob(path, "**/*.md")"#).eval(&r));
        assert!(ok(r#"glob(path, "**/*.{md,html}")"#).eval(&r));
        assert!(!ok(r#"glob(path, "photos/**")"#).eval(&r));
    }

    /// The pattern compiles once, at parse time, so it must be a literal —
    /// and a bad one is a load error rather than a filter that matches
    /// nothing at every row it touches.
    #[test]
    fn a_glob_pattern_must_be_a_literal_and_must_compile() {
        let e = err(r#"glob(path, title)"#);
        assert!(e.contains("argument 2 must be a literal string"), "{e}");
        let e = err(r#"glob(path, "recipes/[")"#);
        assert!(e.contains("bad glob"), "{e}");
    }

    /// `under` and `glob` answer different questions, and the config picks.
    #[test]
    fn glob_is_not_under() {
        let r = TestRow {
            path: "recipes".into(),
            ..Default::default()
        };
        assert!(ok(r#"under(path, "recipes")"#).eval(&r));
        assert!(
            !ok(r#"glob(path, "recipes/**")"#).eval(&r),
            "a directory does not match its own subtree glob"
        );
    }

    #[test]
    fn scopes_conjoin_the_way_a_view_chain_does() {
        let r = TestRow::default();
        assert!(ok(r#"glob(path, "recipes/**") && glob(path, "**/*.md")"#).eval(&r));
        assert!(!ok(r#"glob(path, "recipes/**") && glob(path, "**/*.html")"#).eval(&r));
    }

    #[test]
    fn calls_compose_with_the_rest_of_the_language() {
        let r = TestRow::default();
        assert!(ok(r#"under(path, "recipes") && !draft"#).eval(&r));
        assert!(ok(r#"!under(path, "books")"#).eval(&r));
        assert!(ok(r#"under(path, "books") || year >= 2020"#).eval(&r));
    }

    #[test]
    fn a_call_takes_literals_and_fields_alike() {
        let r = TestRow::default();
        assert!(ok(r#"under("recipes/pasta", "recipes")"#).eval(&r));
        assert!(ok(r#"under(path, title) == false"#).eval(&r));
    }

    #[test]
    fn an_unknown_function_is_a_load_error_that_suggests() {
        let e = err(r#"undor(path, "x")"#);
        assert!(e.contains("unknown function `undor`"), "{e}");
        assert!(e.contains("did you mean `under`"), "{e}");
        assert!(e.contains("known functions: under, glob"), "{e}");
    }

    #[test]
    fn a_calls_arity_and_argument_types_are_checked() {
        let e = err(r#"under(path)"#);
        assert!(e.contains("takes 2 arguments, but 1 were given"), "{e}");
        let e = err(r#"under(path, year)"#);
        assert!(e.contains("argument 2 is string, but `year` is int"), "{e}");
        let e = err(r#"under(tags, "x")"#);
        assert!(
            e.contains("argument 1 is string, but `tags` is list"),
            "{e}"
        );
    }

    /// A call is an operand, so a typo inside one is caught the same way a
    /// bare field reference would be.
    #[test]
    fn an_unknown_field_inside_a_call_is_still_caught() {
        let e = err(r#"under(pth, "x")"#);
        assert!(e.contains("unknown field `pth`"), "{e}");
        assert!(e.contains("did you mean `path`"), "{e}");
    }

    #[test]
    fn a_bool_returning_call_type_checks_as_bool() {
        let e = err(r#"under(path, "x") == "yes""#);
        assert!(e.contains("is bool, but it is compared to a string"), "{e}");
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

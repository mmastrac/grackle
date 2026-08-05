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
    /// A real number, produced by arithmetic and the score functions (§6g
    /// `rank`). Absent from the row schemas — no column is a double — it
    /// exists only as the type of a computed expression.
    Double,
    /// Includes dates, which are ISO-8601 and so order correctly as strings.
    Str,
    List,
    /// Rendered prose as a block sequence (§5f / §6d). Internal to field
    /// expressions, not a schema column. Carries the `truncated` fact when
    /// a `truncate_*` wrapper cuts.
    Content,
    /// A heading tree from `outline(content, max)` (§5f / §6e). Internal to
    /// field expressions; the part layer turns it into `outline_entry` maps.
    Outline,
    /// A CEL map literal (`{"a": 1}`). Keys are int/bool/string; values are
    /// whatever the language already types.
    Map,
    /// Matches any typed value. Only used as a function parameter (e.g.
    /// `to_json`); never a schema column.
    Any,
    /// A string column whose values are a **closed set** — the column is
    /// backed by a Rust enum, so the engine knows every value it can ever
    /// hold. Behaves as [`Type::Str`] in every rule of the language; the
    /// domain buys one thing, and it is the point: comparing the
    /// column to a string outside the set is a load error naming the knowns,
    /// rather than a filter that matches nothing forever.
    ///
    /// `kind == "posts"` — plural, one letter off, silently empty — is the
    /// failure this variant exists to make unwritable.
    Enum(&'static [&'static str]),
}

impl Type {
    /// Int and Double are the arithmetic types; `+ - *` and comparisons mix
    /// them, promoting Int to Double.
    fn is_numeric(self) -> bool {
        matches!(self, Type::Int | Type::Double)
    }

    /// The type as every OTHER rule of the language sees it. An enum column is
    /// a string; nothing about ordering, concatenation, `in` or comparison
    /// changes because a column happens to know its values.
    ///
    /// Every type comparison in the checker goes through this, so a new
    /// domain-carrying variant can never accidentally fail a type match.
    fn scalar(self) -> Type {
        match self {
            Type::Enum(_) => Type::Str,
            t => t,
        }
    }

    /// The closed value set, for the columns that have one.
    fn domain(self) -> Option<&'static [&'static str]> {
        match self {
            Type::Enum(vs) => Some(vs),
            _ => None,
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Type::Bool => "bool",
            Type::Int => "int",
            Type::Double => "double",
            // An enum column IS a string as far as any message about types is
            // concerned — its domain is reported by the domain error, which
            // says something the type error cannot.
            Type::Str | Type::Enum(_) => "string",
            Type::List => "list",
            Type::Content => "content",
            Type::Outline => "outline",
            Type::Map => "map",
            Type::Any => "any",
        })
    }
}

/// One HTML element in a [`Content`] block sequence (§5f / §6d): the element
/// name plus its outer HTML.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Block {
    pub tag: String,
    pub html: String,
}

impl Block {
    pub fn tagged(tag: impl Into<String>, html: impl Into<String>) -> Self {
        Self {
            tag: tag.into(),
            html: html.into(),
        }
    }
}

/// Rendered or authored prose for field expressions (§5f): HTML blocks,
/// markdown source, or plain text, plus whether a `truncate_*` wrapper cut.
/// Bound as `content` when evaluating `fields.NAME`; not a row column.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Content {
    pub body: ContentBody,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum ContentBody {
    Html { blocks: Vec<Block> },
    Markdown(String),
    Text(String),
}

impl Content {
    /// HTML content from typed blocks.
    pub fn html(blocks: Vec<Block>) -> Self {
        Self {
            body: ContentBody::Html { blocks },
            truncated: false,
        }
    }

    /// Tag each HTML fragment (Doc / comrak top-level children). Does not
    /// re-split — one fragment stays one block so truncate prefixes match.
    pub fn from_fragments(frags: Vec<String>) -> Self {
        Self::html(frags.into_iter().map(block_from_fragment).collect())
    }

    /// Parse a full HTML document into blocks, drilling through wrapper `div`s.
    pub fn from_html_document(html: &str) -> Self {
        Self::html(html_to_blocks(html))
    }

    /// Alias for [`Self::from_fragments`]; call sites that mint from `Doc.blocks`.
    pub fn new(blocks: Vec<String>) -> Self {
        Self::from_fragments(blocks)
    }

    pub fn markdown(src: impl Into<String>) -> Self {
        Self {
            body: ContentBody::Markdown(src.into()),
            truncated: false,
        }
    }

    pub fn text(src: impl Into<String>) -> Self {
        Self {
            body: ContentBody::Text(src.into()),
            truncated: false,
        }
    }

    pub fn kind_name(&self) -> &'static str {
        match &self.body {
            ContentBody::Html { .. } => "html",
            ContentBody::Markdown(_) => "markdown",
            ContentBody::Text(_) => "text",
        }
    }

    /// Concatenated HTML when this is HTML content; otherwise empty.
    pub fn html_string(&self) -> String {
        match &self.body {
            ContentBody::Html { blocks } => blocks.iter().map(|b| b.html.as_str()).collect(),
            _ => String::new(),
        }
    }

    /// HTML block list when kind is html.
    pub fn blocks(&self) -> Option<&[Block]> {
        match &self.body {
            ContentBody::Html { blocks } => Some(blocks),
            _ => None,
        }
    }

    /// Keep the first `n` blocks. HTML only; `None` if not HTML.
    pub fn truncate_blocks(&self, n: usize) -> Option<Self> {
        self.truncate_with(Some(n), None)
    }

    /// Keep blocks from the start until visible text would exceed `n`.
    /// HTML only; `None` if not HTML.
    pub fn truncate_chars(&self, n: usize) -> Option<Self> {
        self.truncate_with(None, Some(n))
    }

    /// Keep a prefix of blocks bounded by a block count and/or a char budget
    /// (see `cut_prefix`). HTML only; `None` if not HTML.
    fn truncate_with(&self, max_blocks: Option<usize>, max_chars: Option<usize>) -> Option<Self> {
        let ContentBody::Html { blocks } = &self.body else {
            return None;
        };
        let cut = cut_prefix(blocks, max_blocks, max_chars);
        Some(Self {
            body: ContentBody::Html {
                blocks: blocks[..cut].to_vec(),
            },
            truncated: self.truncated || cut < blocks.len(),
        })
    }

    /// Blocks whose tag is in `tags` (case-insensitive), each as one-block
    /// Content. Markdown renders first; text chunks on blank lines.
    pub fn filter_blocks(&self, tags: &[&str]) -> Vec<Content> {
        let want = tag_set(tags);
        self.blocks_for_filter()
            .into_iter()
            .filter(|b| want.contains(b.tag.as_str()))
            .map(|b| Content {
                body: ContentBody::Html { blocks: vec![b] },
                truncated: self.truncated,
            })
            .collect()
    }

    /// Content keeping only blocks whose tag is in `tags` (case-insensitive),
    /// in document order. Markdown renders first; text chunks on blank lines.
    pub fn keep_blocks(&self, tags: &[&str]) -> Content {
        let want = tag_set(tags);
        let all = self.blocks_for_filter();
        let n = all.len();
        let blocks: Vec<Block> = all
            .into_iter()
            .filter(|b| want.contains(b.tag.as_str()))
            .collect();
        let dropped = blocks.len() < n;
        Content {
            body: ContentBody::Html { blocks },
            truncated: self.truncated || dropped,
        }
    }

    /// Headings from rendered block HTML. Empty if not HTML.
    pub fn headings(&self) -> Vec<Heading> {
        match &self.body {
            ContentBody::Html { blocks } => {
                blocks.iter().filter_map(|b| heading_of(&b.html)).collect()
            }
            _ => Vec::new(),
        }
    }

    /// Heading tree for levels `2..=max`. Empty if not HTML.
    pub fn outline(&self, max: u8) -> Vec<OutlineNode> {
        heading_tree(&self.headings(), 2, max)
    }

    /// Coerce to HTML. Markdown renders and chunks (div-drill); text fails;
    /// HTML is identity.
    pub fn as_html(&self) -> Option<Self> {
        match &self.body {
            ContentBody::Html { .. } => Some(self.clone()),
            ContentBody::Markdown(src) => {
                let html = render_markdown(src);
                Some(Self {
                    body: ContentBody::Html {
                        blocks: html_to_blocks(&html),
                    },
                    truncated: self.truncated,
                })
            }
            ContentBody::Text(_) => None,
        }
    }

    /// Coerce to markdown. Only identity; HTML and text fail.
    pub fn as_markdown(&self) -> Option<Self> {
        match &self.body {
            ContentBody::Markdown(_) => Some(self.clone()),
            ContentBody::Html { .. } | ContentBody::Text(_) => None,
        }
    }

    /// Coerce to plain text. HTML strips tags; markdown renders then strips.
    pub fn as_text(&self) -> Option<Self> {
        match &self.body {
            ContentBody::Text(_) => Some(self.clone()),
            ContentBody::Html { blocks } => {
                let joined: String = blocks.iter().map(|b| b.html.as_str()).collect();
                let t = unescape(&visible_text(&joined));
                Some(Self {
                    body: ContentBody::Text(t),
                    truncated: self.truncated,
                })
            }
            ContentBody::Markdown(_) => self.as_html()?.as_text(),
        }
    }

    /// Whitespace-separated word count after coercing to text.
    pub fn word_count(&self) -> Option<i64> {
        let text = self.as_text()?;
        let ContentBody::Text(s) = &text.body else {
            return None;
        };
        Some(s.split_whitespace().count() as i64)
    }

    /// `href` values from `<a>` tags, document order. Markdown renders first;
    /// plain text yields an empty list.
    pub fn links(&self) -> Vec<String> {
        match self.html_for_scan() {
            Some(html) => attr_values(&html, "a", "href"),
            None => Vec::new(),
        }
    }

    /// `src` values from `<img>` tags, document order. Markdown renders first;
    /// plain text yields an empty list.
    pub fn images(&self) -> Vec<String> {
        match self.html_for_scan() {
            Some(html) => attr_values(&html, "img", "src"),
            None => Vec::new(),
        }
    }

    fn html_for_scan(&self) -> Option<String> {
        match &self.body {
            ContentBody::Html { blocks } => Some(blocks.iter().map(|b| b.html.as_str()).collect()),
            ContentBody::Markdown(_) => self.as_html().map(|c| c.html_string()),
            ContentBody::Text(_) => None,
        }
    }

    fn blocks_for_filter(&self) -> Vec<Block> {
        match &self.body {
            ContentBody::Html { blocks } => blocks.clone(),
            ContentBody::Markdown(_) => self
                .as_html()
                .and_then(|c| c.blocks().map(|b| b.to_vec()))
                .unwrap_or_default(),
            ContentBody::Text(src) => text_to_blocks(src),
        }
    }
}

fn tag_set(tags: &[&str]) -> std::collections::BTreeSet<String> {
    tags.iter().map(|t| t.to_ascii_lowercase()).collect()
}

/// Tag a pre-split HTML fragment (opening element name, or `"p"` for bare text).
fn block_from_fragment(html: String) -> Block {
    let tag = opening_tag(&html).unwrap_or_else(|| "p".into());
    Block { tag, html }
}

/// Blank-line paragraphs → `<p>…</p>` blocks (markdown-style text chunking).
fn text_to_blocks(src: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, blocks: &mut Vec<Block>| {
        let t = cur.trim();
        if t.is_empty() {
            cur.clear();
            return;
        }
        let html = format!("<p>{}</p>", escape_text(t));
        blocks.push(Block::tagged("p", html));
        cur.clear();
    };
    for line in src.lines() {
        if line.trim().is_empty() {
            flush(&mut cur, &mut blocks);
        } else {
            if !cur.is_empty() {
                cur.push('\n');
            }
            cur.push_str(line);
        }
    }
    flush(&mut cur, &mut blocks);
    blocks
}

fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Walk HTML; drill through wrapper `div`s; emit one [`Block`] per other element.
fn html_to_blocks(html: &str) -> Vec<Block> {
    let mut out = Vec::new();
    collect_html_blocks(html, &mut out);
    out
}

fn collect_html_blocks(html: &str, out: &mut Vec<Block>) {
    let mut i = 0;
    let n = html.len();
    while i < n {
        while i < n && html.as_bytes()[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= n {
            break;
        }
        if !html[i..].starts_with('<') {
            let start = i;
            while i < n && !html[i..].starts_with('<') {
                i += 1;
            }
            let text = html[start..i].trim();
            if !text.is_empty() {
                out.push(Block::tagged("p", format!("<p>{}</p>", escape_text(text))));
            }
            continue;
        }
        if html[i..].starts_with("<!--") {
            i = html[i..].find("-->").map(|e| i + e + 3).unwrap_or(n);
            continue;
        }
        if html[i..].starts_with("<!") {
            i = html[i..].find('>').map(|e| i + e + 1).unwrap_or(n);
            continue;
        }
        let Some((tag, end)) = take_element(html, i) else {
            i += 1;
            continue;
        };
        let outer = html[i..end].to_string();
        i = end;
        if tag == "div" {
            if let Some(inner) = element_inner(&outer) {
                collect_html_blocks(inner, out);
            }
            continue;
        }
        out.push(Block { tag, html: outer });
    }
}

fn opening_tag(html: &str) -> Option<String> {
    let h = html.trim_start();
    if !h.starts_with('<') {
        return None;
    }
    let rest = &h[1..];
    if rest.starts_with('/') || rest.starts_with('!') {
        return None;
    }
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    if name.is_empty() {
        None
    } else {
        Some(name.to_ascii_lowercase())
    }
}

fn is_void_tag(tag: &str) -> bool {
    matches!(
        tag,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

/// Outer element starting at `start` (must be `<`); returns (tag, end_index).
fn take_element(html: &str, start: usize) -> Option<(String, usize)> {
    let tag = opening_tag(&html[start..])?;
    let after_lt = start + 1;
    let open_end = html[after_lt..].find('>').map(|e| after_lt + e + 1)?;
    let open = &html[start..open_end];
    if is_void_tag(&tag) || open.trim_end().ends_with("/>") {
        return Some((tag, open_end));
    }
    let mut depth = 1usize;
    let mut i = open_end;
    let open_pat = format!("<{tag}");
    let close_pat = format!("</{tag}");
    while i < html.len() {
        if html[i..].starts_with(&close_pat) {
            let next = html[i + close_pat.len()..].chars().next();
            if matches!(next, Some(c) if c == '>' || c.is_whitespace()) {
                depth -= 1;
                i = html[i..].find('>').map(|e| i + e + 1).unwrap_or(html.len());
                if depth == 0 {
                    return Some((tag, i));
                }
                continue;
            }
        }
        if html[i..].starts_with(&open_pat) {
            let next = html[i + open_pat.len()..].chars().next();
            if matches!(next, Some(c) if c == '>' || c.is_whitespace() || c == '/') {
                let end = html[i..].find('>').map(|e| i + e + 1).unwrap_or(html.len());
                let slice = &html[i..end];
                if !(is_void_tag(&tag) || slice.trim_end().ends_with("/>")) {
                    depth += 1;
                }
                i = end;
                continue;
            }
        }
        i += 1;
    }
    Some((tag, html.len()))
}

fn element_inner(outer: &str) -> Option<&str> {
    let open_end = outer.find('>')? + 1;
    let close_start = outer.rfind("</")?;
    if close_start < open_end {
        return None;
    }
    Some(&outer[open_end..close_start])
}

/// Values of `attr` on opening tags named `tag` (`<a href=…>`, `<img src=…>`).
fn attr_values(html: &str, tag: &str, attr: &str) -> Vec<String> {
    let needle = format!("<{tag}");
    let attr_eq = format!("{attr}=");
    let mut out = Vec::new();
    let mut i = 0;
    while i < html.len() {
        let Some(rel) = html[i..].find(&needle) else {
            break;
        };
        i += rel + needle.len();
        let next = html[i..].chars().next();
        if !matches!(next, Some(c) if c.is_whitespace() || c == '>' || c == '/') {
            continue;
        }
        let end = html[i..].find('>').map(|e| i + e).unwrap_or(html.len());
        let open = &html[i..end];
        if let Some(v) = quoted_attr(open, &attr_eq) {
            out.push(unescape(&v));
        }
        i = end.saturating_add(1).min(html.len());
    }
    out
}

fn quoted_attr(open: &str, attr_eq: &str) -> Option<String> {
    let i = open.find(attr_eq)? + attr_eq.len();
    let rest = &open[i..];
    let quote = rest.chars().next().filter(|&c| c == '"' || c == '\'')?;
    let body = &rest[quote.len_utf8()..];
    let end = body.find(quote)?;
    Some(body[..end].to_string())
}

/// Fixed minimal comrak options for `as_html(markdown(…))` in the language
/// crate, not the site's full kramdown stand-in (that stays in core).
fn render_markdown(src: &str) -> String {
    let mut opts = comrak::Options::default();
    opts.extension.strikethrough = true;
    opts.extension.table = true;
    opts.extension.autolink = true;
    let arena = comrak::Arena::new();
    let root = comrak::parse_document(&arena, src, &opts);
    let mut out = String::new();
    comrak::format_html(root, &opts, &mut out).unwrap_or_default();
    out
}

/// One heading in rendered HTML.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Heading {
    pub level: u8,
    pub id: String,
    pub text: String,
}

/// One node in an `outline(content, max)` tree.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct OutlineNode {
    pub label: String,
    pub url: Option<String>,
    pub children: Vec<OutlineNode>,
}

fn heading_of(html: &str) -> Option<Heading> {
    let h = html.trim_start();
    let rest = h.strip_prefix("<h")?;
    let level = rest.bytes().next().filter(u8::is_ascii_digit)? - b'0';
    if !(1..=6).contains(&level) {
        return None;
    }
    let i = h.find("id=\"")? + 4;
    let id = h[i..].split('"').next()?.to_string();
    let text = unescape(&visible_text(h));
    Some(Heading { level, id, text })
}

fn visible_text(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

/// Nest headings by level; a jump (h2 → h4) nests under the nearest shallower.
pub fn heading_tree(hs: &[Heading], min: u8, max: u8) -> Vec<OutlineNode> {
    fn build(hs: &[&Heading], i: &mut usize, parent: u8) -> Vec<OutlineNode> {
        let mut out = Vec::new();
        while *i < hs.len() {
            let h = hs[*i];
            if h.level <= parent {
                break;
            }
            *i += 1;
            let children = build(hs, i, h.level);
            out.push(OutlineNode {
                label: h.text.clone(),
                url: Some(format!("#{}", h.id)),
                children,
            });
        }
        out
    }
    let kept: Vec<&Heading> = hs
        .iter()
        .filter(|h| h.level >= min && h.level <= max)
        .collect();
    build(&kept, &mut 0, 0)
}

/// Block index where a budget cuts: walk from the start, stop at the first
/// limit. `max_chars` counts visible text (outside tags); the block that
/// would exceed it is dropped whole, but at least one block is always kept.
fn cut_prefix(blocks: &[Block], max_blocks: Option<usize>, max_chars: Option<usize>) -> usize {
    let mut cut = blocks.len();
    let mut chars = 0usize;
    for (i, b) in blocks.iter().enumerate() {
        if let Some(mb) = max_blocks {
            if i >= mb {
                cut = i;
                break;
            }
        }
        if let Some(mc) = max_chars {
            chars += text_len(&b.html);
            if chars > mc && i > 0 {
                cut = i;
                break;
            }
        }
    }
    cut
}

/// Visible text length of an HTML fragment: characters outside tags.
/// Entity-naive (`&amp;` counts as 5), which errs on the side of keeping
/// summaries short: fine for a budget, wrong for typography.
fn text_len(html: &str) -> usize {
    let mut n = 0;
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => n += 1,
            _ => {}
        }
    }
    n
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(untagged)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Double(f64),
    Str(String),
    /// CEL list: strings (tags, links) or Content (`filter_blocks` results).
    List(Vec<Value>),
    Content(Content),
    Outline(Vec<OutlineNode>),
    /// CEL map: key/value pairs in literal order. Keys are int/bool/string.
    Map(Vec<(Value, Value)>),
    Null,
}

impl Value {
    /// A list of strings (tags, links, images).
    pub fn str_list(items: impl IntoIterator<Item = String>) -> Self {
        Value::List(items.into_iter().map(Value::Str).collect())
    }

    /// String elements of a list value (non-strings skipped).
    pub fn as_str_list(&self) -> Vec<String> {
        match self {
            Value::List(items) => items
                .iter()
                .filter_map(|v| match v {
                    Value::Str(s) => Some(s.clone()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// As an error message names it.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Bool(_) => "a bool",
            Value::Int(_) => "an int",
            Value::Double(_) => "a double",
            Value::Str(_) => "a string",
            Value::List(_) => "a list",
            Value::Content(_) => "content",
            Value::Outline(_) => "an outline",
            Value::Map(_) => "a map",
            Value::Null => "null",
        }
    }

    /// This value as an `f64`, for arithmetic and ranking. Int widens; a
    /// non-number (including `Null`, so a missing field) is not a score.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Double(d) => Some(*d),
            _ => None,
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
            (Value::Double(x), Value::Double(y)) => x.total_cmp(y),
            (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
            (Value::List(x), Value::List(y)) => cmp_str_lists(x, y),
            // Content is not a sort key; mixed types cannot arise for columns.
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
            Value::Double(d) => *d != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::List(v) => !v.is_empty(),
            Value::Content(_) | Value::Outline(_) | Value::Map(_) => false,
            Value::Null => false,
        }
    }
}

fn cmp_str_lists(a: &[Value], b: &[Value]) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    let all_str = |v: &[Value]| v.iter().all(|x| matches!(x, Value::Str(_)));
    if !all_str(a) || !all_str(b) {
        return Equal;
    }
    for (x, y) in a.iter().zip(b) {
        let (Value::Str(u), Value::Str(v)) = (x, y) else {
            unreachable!();
        };
        match u.cmp(v) {
            Equal => continue,
            o => return o,
        }
    }
    a.len().cmp(&b.len())
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
    Double(f64),
    Str(String),
    Bool(bool),
}

impl Lit {
    fn ty(&self) -> Type {
        match self {
            Lit::Int(_) => Type::Int,
            Lit::Double(_) => Type::Double,
            Lit::Str(_) => Type::Str,
            Lit::Bool(_) => Type::Bool,
        }
    }
    fn value(&self) -> Value {
        match self {
            Lit::Int(i) => Value::Int(*i),
            Lit::Double(d) => Value::Double(*d),
            Lit::Str(s) => Value::Str(s.clone()),
            Lit::Bool(b) => Value::Bool(*b),
        }
    }
}

/// The arithmetic operators, for `rank` expressions (§6g slice 2). Division
/// is deliberately absent: no ranking formula the design records needs it,
/// and leaving it out keeps "can this divide by zero" from ever being a
/// question a config can raise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArithOp {
    Add,
    Sub,
    Mul,
}

impl ArithOp {
    fn apply(self, a: f64, b: f64) -> f64 {
        match self {
            ArithOp::Add => a + b,
            ArithOp::Sub => a - b,
            ArithOp::Mul => a * b,
        }
    }
    fn as_str(self) -> &'static str {
        match self {
            ArithOp::Add => "+",
            ArithOp::Sub => "-",
            ArithOp::Mul => "*",
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
    /// Unary minus, so distance functions rank with a sign (`-levenshtein`).
    Neg(Box<Operand>),
    /// Arithmetic, for `rank` (§6g): `similarity(self, candidate) - 0.01 * gap`.
    Arith(Box<Operand>, ArithOp, Box<Operand>),
    /// CEL map literal: `{key: value, …}`.
    Map(Vec<(Operand, Operand)>),
    /// CEL list index: `links(content)[0]`. Out of range is Null at eval.
    Index(Box<Operand>, Box<Operand>),
    /// CEL conditional: `cond ? then : else` (right-associative via nested else).
    Cond(Box<Expr>, Box<Operand>, Box<Operand>),
}

#[derive(Debug, Clone)]
enum Expr {
    True,
    Truthy(Operand),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Cmp(Operand, Op, Operand),
    /// `"rust" in tags`, and — with a field on the left — `candidate in earlier`
    /// (§6g name membership: is this row in that relation's finished list).
    In(Operand, Operand),
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
    /// When set, further arguments of this type are allowed after `params`
    /// (at least one required). Used by `filter_blocks` / `keep_blocks`.
    rest: Option<Type>,
    returns: Type,
    /// Whatever the function can work out once, at parse time, from its
    /// literal arguments. A glob is a regex; compiling one per row per filter
    /// is not a trade worth making, and a pattern that varies per row is not
    /// a thing anyone wants.
    prepare: fn(&[Operand]) -> Result<Prepared>,
    /// Evaluate against argument values, with access to the `Ctx` for the
    /// score functions that need data no argument carries (embedding vectors,
    /// a search index). A pure function ignores the ctx.
    eval: fn(&Prepared, &[Value], &dyn Ctx) -> Value,
}

/// The out-of-band data the score functions read (§6g slice 2). `self` and
/// `candidate` reach a function as their URLs (a `Str` argument); the ctx
/// turns a URL into a vector, a date, whatever the function needs. Kept a
/// trait so the language crate stays free of the embedding machinery — the
/// engine supplies the real implementation at build time, and view filters,
/// which call no score function, pass `NoCtx`.
pub trait Ctx {
    /// Cosine similarity of two rows' body embeddings, by URL. `None` when
    /// either row has no vector — the pair simply does not rank.
    fn similarity(&self, _a: &str, _b: &str) -> Option<f64> {
        None
    }
    /// Whole-years between two rows' dates, by URL; `None` when either is
    /// undated.
    fn year_gap(&self, _a: &str, _b: &str) -> Option<f64> {
        None
    }
}

/// The context for an expression that calls no score function — every view
/// filter, and every relation still on the built-in embedding order.
pub struct NoCtx;
impl Ctx for NoCtx {}

/// The two URL arguments a row-pair score function receives, or `None` if
/// either is not a string (which the type checker forbids, so this only
/// guards against a bug). A `None` result — here or from the ctx — makes the
/// pair unrankable rather than a crash, the same "missing input drops the
/// candidate" the filters already have.
fn url_pair(args: &[Value]) -> Option<(&str, &str)> {
    match (args.first(), args.get(1)) {
        (Some(Value::Str(a)), Some(Value::Str(b))) => Some((a, b)),
        _ => None,
    }
}

/// The first argument as a string, `None` if absent or not a `Str` — the
/// shape every single-string constructor shares.
fn str_arg(args: &[Value]) -> Option<&str> {
    match args.first() {
        Some(Value::Str(s)) => Some(s),
        _ => None,
    }
}

/// The first argument as content, `None` otherwise.
fn content_arg(args: &[Value]) -> Option<&Content> {
    match args.first() {
        Some(Value::Content(c)) => Some(c),
        _ => None,
    }
}

/// The first two arguments as (content, int), `None` unless both fit.
fn content_int_arg(args: &[Value]) -> Option<(&Content, i64)> {
    match (args.first(), args.get(1)) {
        (Some(Value::Content(c)), Some(Value::Int(n))) => Some((c, *n)),
        _ => None,
    }
}

fn eval_embedding_similarity(_: &Prepared, args: &[Value], ctx: &dyn Ctx) -> Value {
    url_pair(args)
        .and_then(|(a, b)| ctx.similarity(a, b))
        .map_or(Value::Null, Value::Double)
}

fn eval_year_gap(_: &Prepared, args: &[Value], ctx: &dyn Ctx) -> Value {
    url_pair(args)
        .and_then(|(a, b)| ctx.year_gap(a, b))
        .map_or(Value::Null, Value::Double)
}

/// Edit distance between two strings — pure, no ctx. Wears a minus sign in a
/// `rank` (`-levenshtein(...)`) because bigger always wins (§6g).
fn eval_levenshtein(_: &Prepared, args: &[Value], _: &dyn Ctx) -> Value {
    url_pair(args).map_or(Value::Null, |(a, b)| Value::Int(levenshtein(a, b) as i64))
}

fn eval_truncate_blocks(_: &Prepared, args: &[Value], _: &dyn Ctx) -> Value {
    content_int_arg(args)
        .and_then(|(c, n)| c.truncate_blocks(n.max(0) as usize))
        .map_or(Value::Null, Value::Content)
}

fn eval_truncate_chars(_: &Prepared, args: &[Value], _: &dyn Ctx) -> Value {
    content_int_arg(args)
        .and_then(|(c, n)| c.truncate_chars(n.max(0) as usize))
        .map_or(Value::Null, Value::Content)
}

fn eval_outline(_: &Prepared, args: &[Value], _: &dyn Ctx) -> Value {
    content_int_arg(args)
        .filter(|(c, _)| c.blocks().is_some())
        .map_or(Value::Null, |(c, n)| {
            Value::Outline(c.outline(n.clamp(1, 6) as u8))
        })
}

fn eval_html(_: &Prepared, args: &[Value], _: &dyn Ctx) -> Value {
    str_arg(args).map_or(Value::Null, |s| {
        Value::Content(Content::from_html_document(s))
    })
}

fn eval_markdown(_: &Prepared, args: &[Value], _: &dyn Ctx) -> Value {
    str_arg(args).map_or(Value::Null, |s| {
        Value::Content(Content::markdown(s.to_string()))
    })
}

fn eval_text_ctor(_: &Prepared, args: &[Value], _: &dyn Ctx) -> Value {
    str_arg(args).map_or(Value::Null, |s| {
        Value::Content(Content::text(s.to_string()))
    })
}

fn eval_kind(_: &Prepared, args: &[Value], _: &dyn Ctx) -> Value {
    content_arg(args).map_or(Value::Null, |c| Value::Str(c.kind_name().into()))
}

fn eval_as_html(_: &Prepared, args: &[Value], _: &dyn Ctx) -> Value {
    content_arg(args)
        .and_then(Content::as_html)
        .map_or(Value::Null, Value::Content)
}

fn eval_as_markdown(_: &Prepared, args: &[Value], _: &dyn Ctx) -> Value {
    content_arg(args)
        .and_then(Content::as_markdown)
        .map_or(Value::Null, Value::Content)
}

fn eval_as_text(_: &Prepared, args: &[Value], _: &dyn Ctx) -> Value {
    content_arg(args)
        .and_then(Content::as_text)
        .map_or(Value::Null, Value::Content)
}

fn eval_word_count(_: &Prepared, args: &[Value], _: &dyn Ctx) -> Value {
    content_arg(args)
        .and_then(Content::word_count)
        .map_or(Value::Null, Value::Int)
}

fn eval_links(_: &Prepared, args: &[Value], _: &dyn Ctx) -> Value {
    content_arg(args).map_or(Value::Null, |c| Value::str_list(c.links()))
}

fn eval_images(_: &Prepared, args: &[Value], _: &dyn Ctx) -> Value {
    content_arg(args).map_or(Value::Null, |c| Value::str_list(c.images()))
}

fn eval_filter_blocks(_: &Prepared, args: &[Value], _: &dyn Ctx) -> Value {
    let Some(c) = content_arg(args) else {
        return Value::Null;
    };
    let tags = str_args_from(1, args);
    if tags.is_empty() {
        return Value::Null;
    }
    let tag_refs: Vec<&str> = tags.iter().map(|s| s.as_str()).collect();
    Value::List(
        c.filter_blocks(&tag_refs)
            .into_iter()
            .map(Value::Content)
            .collect(),
    )
}

fn eval_keep_blocks(_: &Prepared, args: &[Value], _: &dyn Ctx) -> Value {
    let Some(c) = content_arg(args) else {
        return Value::Null;
    };
    let tags = str_args_from(1, args);
    if tags.is_empty() {
        return Value::Null;
    }
    let tag_refs: Vec<&str> = tags.iter().map(|s| s.as_str()).collect();
    Value::Content(c.keep_blocks(&tag_refs))
}

fn str_args_from(start: usize, args: &[Value]) -> Vec<String> {
    args[start..]
        .iter()
        .filter_map(|v| match v {
            Value::Str(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

fn eval_to_json(_: &Prepared, args: &[Value], _: &dyn Ctx) -> Value {
    match args.first() {
        Some(v) => Value::Str(value_to_json(v)),
        None => Value::Null,
    }
}

/// JSON encoding of a language value. Map keys become strings (JSON objects
/// have no int/bool keys); Content/Outline are opaque placeholders.
fn value_to_json(v: &Value) -> String {
    serde_json::to_string(&value_to_json_value(v)).unwrap_or_else(|_| "null".into())
}

fn value_to_json_value(v: &Value) -> serde_json::Value {
    use serde_json::{json, Map as JsonMap, Value as J};
    match v {
        Value::Bool(b) => J::Bool(*b),
        Value::Int(i) => json!(i),
        Value::Double(d) => json!(d),
        Value::Str(s) => J::String(s.clone()),
        Value::List(items) => J::Array(items.iter().map(value_to_json_value).collect()),
        Value::Map(entries) => {
            let mut m = JsonMap::new();
            for (k, val) in entries {
                m.insert(map_key_json(k), value_to_json_value(val));
            }
            J::Object(m)
        }
        Value::Content(c) => json!({
            "kind": c.kind_name(),
            "truncated": c.truncated,
        }),
        Value::Outline(_) => J::String("<outline>".into()),
        Value::Null => J::Null,
    }
}

fn map_key_json(k: &Value) -> String {
    match k {
        Value::Str(s) => s.clone(),
        Value::Int(i) => i.to_string(),
        Value::Bool(b) => b.to_string(),
        other => other.type_name().to_string(),
    }
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

impl Func {
    /// A function with no parse-time preparation and no rest args — every
    /// entry below except `under`/`glob`, which carry their own `eval`/`prepare`.
    const fn simple(
        name: &'static str,
        params: &'static [Type],
        returns: Type,
        eval: fn(&Prepared, &[Value], &dyn Ctx) -> Value,
    ) -> Func {
        Func {
            name,
            params,
            rest: None,
            returns,
            prepare: no_prep,
            eval,
        }
    }

    /// [`Func::simple`] with a `rest` type: further args of that type after `params`.
    const fn simple_rest(
        name: &'static str,
        params: &'static [Type],
        rest: Type,
        returns: Type,
        eval: fn(&Prepared, &[Value], &dyn Ctx) -> Value,
    ) -> Func {
        Func {
            name,
            params,
            rest: Some(rest),
            returns,
            prepare: no_prep,
            eval,
        }
    }
}

const FUNCS: &[Func] = &[
    Func {
        name: "under",
        params: &[Type::Str, Type::Str],
        rest: None,
        returns: Type::Bool,
        prepare: no_prep,
        eval: |_, args, _| match (&args[0], &args[1]) {
            (Value::Str(p), Value::Str(base)) => Value::Bool(path_under(p, base)),
            // A null path is under nothing, which is what a row with no path
            // should mean rather than an error at query time.
            _ => Value::Bool(false),
        },
    },
    Func {
        name: "glob",
        params: &[Type::Str, Type::Str],
        rest: None,
        returns: Type::Bool,
        prepare: |args| {
            let pat = literal_arg(args, 1, "glob")?;
            Ok(Prepared::Glob(
                globset::Glob::new(pat)
                    .map_err(|e| anyhow!("bad glob {pat:?}: {e}"))?
                    .compile_matcher(),
            ))
        },
        eval: |prep, args, _| match (prep, &args[0]) {
            (Prepared::Glob(m), Value::Str(p)) => Value::Bool(m.is_match(p)),
            _ => Value::Bool(false),
        },
    },
    // The §6g score functions. `self` and `candidate` reach them as their
    // URLs (Str), which the ctx resolves to a vector or a date. Registered
    // in the one shared table (§5f): a view filter *could* name them, but its
    // single-row schema has no `self`/`candidate`, so the argument would not
    // resolve — the language is one, the environment gates the reach.
    Func::simple(
        "embedding_similarity",
        &[Type::Str, Type::Str],
        Type::Double,
        eval_embedding_similarity,
    ),
    Func::simple(
        "year_gap",
        &[Type::Str, Type::Str],
        Type::Double,
        eval_year_gap,
    ),
    Func::simple(
        "levenshtein",
        &[Type::Str, Type::Str],
        Type::Int,
        eval_levenshtein,
    ),
    // §5f field derivers: wrappers on Content. Compose for both budgets
    // (`truncate_chars(truncate_blocks(content, 4), 700)`) rather than a
    // map-literal options bag. Facts ride on the value (`truncated`).
    // HTML-only; coerce with as_html first when the source is markdown.
    Func::simple(
        "truncate_blocks",
        &[Type::Content, Type::Int],
        Type::Content,
        eval_truncate_blocks,
    ),
    Func::simple(
        "truncate_chars",
        &[Type::Content, Type::Int],
        Type::Content,
        eval_truncate_chars,
    ),
    // §6e heading ToC: max level is positional (3 ≡ today's h2–h3 window);
    // min stays 2 so the page title is never an entry. HTML only.
    Func::simple(
        "outline",
        &[Type::Content, Type::Int],
        Type::Outline,
        eval_outline,
    ),
    // Content constructors and kind coercion (§5f).
    Func::simple("html", &[Type::Str], Type::Content, eval_html),
    Func::simple("markdown", &[Type::Str], Type::Content, eval_markdown),
    Func::simple("text", &[Type::Str], Type::Content, eval_text_ctor),
    Func::simple("kind", &[Type::Content], Type::Str, eval_kind),
    Func::simple("as_html", &[Type::Content], Type::Content, eval_as_html),
    Func::simple(
        "as_markdown",
        &[Type::Content],
        Type::Content,
        eval_as_markdown,
    ),
    Func::simple("as_text", &[Type::Content], Type::Content, eval_as_text),
    Func::simple("word_count", &[Type::Content], Type::Int, eval_word_count),
    Func::simple("links", &[Type::Content], Type::List, eval_links),
    Func::simple("images", &[Type::Content], Type::List, eval_images),
    Func::simple_rest(
        "filter_blocks",
        &[Type::Content],
        Type::Str,
        Type::List,
        eval_filter_blocks,
    ),
    Func::simple_rest(
        "keep_blocks",
        &[Type::Content],
        Type::Str,
        Type::Content,
        eval_keep_blocks,
    ),
    Func::simple("to_json", &[Type::Any], Type::Str, eval_to_json),
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
    Float(f64),
    Str(String),
    Star,
    Plus,
    Minus,
    Bang,
    And,
    Or,
    Cmp(Op),
    In,
    LParen,
    RParen,
    Comma,
    Question,
    Colon,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
}

fn lex(src: &str) -> Result<Vec<Tok>> {
    let b: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i];
        match c {
            c if c.is_whitespace() => i += 1,
            // Single-character punctuation tokens, one per char.
            '(' | ')' | '{' | '}' | '[' | ']' | ',' | '?' | ':' | '*' | '+' => {
                out.push(match c {
                    '(' => Tok::LParen,
                    ')' => Tok::RParen,
                    '{' => Tok::LBrace,
                    '}' => Tok::RBrace,
                    '[' => Tok::LBracket,
                    ']' => Tok::RBracket,
                    ',' => Tok::Comma,
                    '?' => Tok::Question,
                    ':' => Tok::Colon,
                    '*' => Tok::Star,
                    _ => Tok::Plus,
                });
                i += 1;
            }
            // Always an operator token; a negative literal is unary minus over
            // a number, folded by the parser. Context-free is the point — the
            // lexer never has to guess whether `-` continues an operand.
            '-' => {
                out.push(Tok::Minus);
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
            c if c.is_ascii_digit() => {
                let start = i;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
                // A `.` is a fraction only when a digit follows: `0.01` is a
                // float, but `2.foo` is not (and does not arise). Without the
                // digit check a trailing dot would swallow a field access.
                let is_float =
                    b.get(i) == Some(&'.') && b.get(i + 1).is_some_and(|d| d.is_ascii_digit());
                if is_float {
                    i += 1; // the '.'
                    while i < b.len() && b[i].is_ascii_digit() {
                        i += 1;
                    }
                    let s: String = b[start..i].iter().collect();
                    out.push(Tok::Float(s.parse()?));
                } else {
                    let s: String = b[start..i].iter().collect();
                    out.push(Tok::Int(s.parse()?));
                }
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
        // A boolean group: `(draft || hidden)`. Distinct from a parenthesized
        // *arithmetic* operand `(a + b)`, which parse_atom handles — the two
        // never collide because this branch only fires at the boolean level.
        if self.eat(&Tok::LParen) {
            let e = self.parse_or()?;
            if !self.eat(&Tok::RParen) {
                bail!("expected `)`");
            }
            // A leading `(` is a boolean group; a parenthesised *arithmetic*
            // operand on the left of a comparison (`(a + b) > c`) is valid CEL
            // this evaluator does not parse yet — name the shape rather than
            // let it fall through to a misleading "trailing tokens".
            if matches!(self.peek(), Some(Tok::Cmp(_)) | Some(Tok::In)) {
                bail!(
                    "a parenthesised expression on the left of a comparison is \
                     valid CEL but not supported yet — lift it into a field or \
                     a rank term"
                );
            }
            return Ok(e);
        }
        match self.peek() {
            Some(Tok::Star) => {
                self.pos += 1;
                return Ok(Expr::True);
            }
            Some(Tok::Ident(n)) if n == "true" => {
                self.pos += 1;
                return Ok(Expr::True);
            }
            Some(Tok::Ident(n)) if n == "false" => {
                self.pos += 1;
                return Ok(Expr::Not(Box::new(Expr::True)));
            }
            None => bail!("unexpected end of expression"),
            _ => {}
        }
        // Everything else is an operand, then optionally a comparison or a
        // membership test. A bare operand standing alone is a truthiness test
        // (`!draft`, `description`) — but only a field or call can be one; a
        // lone literal is the `"rust"` -without-`in` mistake.
        let left = self.parse_arith()?;
        match self.peek().cloned() {
            Some(Tok::Cmp(op)) => {
                self.pos += 1;
                let right = self.parse_arith()?;
                Ok(Expr::Cmp(left, op, right))
            }
            Some(Tok::In) => {
                self.pos += 1;
                let right = self.parse_atom()?;
                Ok(Expr::In(left, right))
            }
            _ => match left {
                Operand::Field(_) | Operand::Call(..) | Operand::Index(..) => {
                    Ok(Expr::Truthy(left))
                }
                Operand::Lit(_) => {
                    bail!("a literal is only valid on the left of `in` (as in `\"rust\" in tags`)")
                }
                _ => bail!("an arithmetic expression is not a condition on its own"),
            },
        }
    }

    /// Arithmetic, lowest precedence first: `+`/`-`, then `*`, then unary
    /// minus, then an atom. Present for `rank` (§6g); a boolean filter reaches
    /// it too, so `year >= 2020` runs `2020` through here as a bare atom.
    fn parse_arith(&mut self) -> Result<Operand> {
        let mut lhs = self.parse_mul()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Plus) => ArithOp::Add,
                Some(Tok::Minus) => ArithOp::Sub,
                _ => break,
            };
            self.pos += 1;
            let rhs = self.parse_mul()?;
            lhs = Operand::Arith(Box::new(lhs), op, Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_mul(&mut self) -> Result<Operand> {
        let mut lhs = self.parse_neg()?;
        while self.eat(&Tok::Star) {
            let rhs = self.parse_neg()?;
            lhs = Operand::Arith(Box::new(lhs), ArithOp::Mul, Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_neg(&mut self) -> Result<Operand> {
        if self.eat(&Tok::Minus) {
            return Ok(Operand::Neg(Box::new(self.parse_neg()?)));
        }
        self.parse_atom()
    }

    /// The leaf of an operand: a parenthesized arithmetic group, a call, a
    /// field, a literal, or a map — then optional `[index]` suffixes.
    fn parse_atom(&mut self) -> Result<Operand> {
        let mut base = match self.next() {
            Some(Tok::LParen) => {
                let e = self.parse_arith()?;
                if !self.eat(&Tok::RParen) {
                    bail!("expected `)`");
                }
                e
            }
            Some(Tok::Ident(name)) if name == "true" || name == "false" => {
                Operand::Lit(Lit::Bool(name == "true"))
            }
            Some(Tok::Ident(name)) => self.parse_call_or_field(name)?,
            Some(Tok::Str(s)) => Operand::Lit(Lit::Str(s)),
            Some(Tok::Int(i)) => Operand::Lit(Lit::Int(i)),
            Some(Tok::Float(f)) => Operand::Lit(Lit::Double(f)),
            Some(Tok::LBrace) => self.parse_map()?,
            Some(t) => bail!("expected a field, literal or call, found {t:?}"),
            None => bail!("expected an operand"),
        };
        while self.eat(&Tok::LBracket) {
            let idx = self.parse_arith()?;
            if !self.eat(&Tok::RBracket) {
                bail!("expected `]` after an index");
            }
            base = Operand::Index(Box::new(base), Box::new(idx));
        }
        Ok(base)
    }

    /// CEL map literal: `{k: v, …}`. Trailing commas are allowed.
    fn parse_map(&mut self) -> Result<Operand> {
        let mut entries = Vec::new();
        if self.eat(&Tok::RBrace) {
            return Ok(Operand::Map(entries));
        }
        loop {
            let key = self.parse_arith()?;
            if !self.eat(&Tok::Colon) {
                bail!("expected `:` between a map key and its value");
            }
            let val = self.parse_arith()?;
            entries.push((key, val));
            if self.eat(&Tok::RBrace) {
                break;
            }
            if !self.eat(&Tok::Comma) {
                bail!("expected `,` or `}}` in a map literal");
            }
            if self.eat(&Tok::RBrace) {
                break;
            }
        }
        Ok(Operand::Map(entries))
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
                args.push(self.parse_arith()?);
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
        let min = func.params.len() + usize::from(func.rest.is_some());
        let ok = if func.rest.is_some() {
            args.len() >= min
        } else {
            args.len() == func.params.len()
        };
        if !ok {
            let want = match func.rest {
                Some(_) => format!("at least {min}"),
                None => format!("{}", func.params.len()),
            };
            bail!(
                "`{name}` takes {want} argument{}, but {} were given",
                if min == 1 && func.rest.is_none() {
                    ""
                } else {
                    "s"
                },
                args.len()
            );
        }
        let prep = (func.prepare)(&args)?;
        Ok(Operand::Call(func, args, prep))
    }
}

/// Edit distance. Registered as a CEL function (`-levenshtein(a, b)` ranks
/// relations, §6g) and used by every "did you mean" this engine offers —
/// unknown field, unknown function, and now an unknown link query key that
/// looks like an axis. `pub` so that stays ONE implementation.
pub fn levenshtein(a: &str, b: &str) -> usize {
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
            Operand::Lit(Lit::Double(d)) => write!(f, "{d}"),
            Operand::Lit(Lit::Bool(b)) => write!(f, "{b}"),
            Operand::Neg(o) => write!(f, "-{o}"),
            Operand::Arith(a, op, b) => write!(f, "{a} {} {b}", op.as_str()),
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
            Operand::Map(entries) => {
                f.write_str("{")?;
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{k}: {v}")?;
                }
                f.write_str("}")
            }
            Operand::Index(base, idx) => write!(f, "{base}[{idx}]"),
            Operand::Cond(_, t, e) => write!(f, "... ? {t} : {e}"),
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
                if *want != Type::Any && got.scalar() != want.scalar() {
                    bail!(
                        "`{}` argument {} is {want}, but `{arg}` is {got}",
                        func.name,
                        i + 1
                    );
                }
            }
            if let Some(want) = func.rest {
                for (i, arg) in args.iter().enumerate().skip(func.params.len()) {
                    let got = operand_type(arg, schema)?;
                    if want != Type::Any && got.scalar() != want.scalar() {
                        bail!(
                            "`{}` argument {} is {want}, but `{arg}` is {got}",
                            func.name,
                            i + 1
                        );
                    }
                }
            }
            Ok(func.returns)
        }
        // Arithmetic is numbers only, and Int mixed with Double is Double —
        // the one promotion, so `candidate.date` (dropped) never quietly
        // stringifies into a score. A non-number is the error a rank typo
        // makes: `rank = "title"`.
        Operand::Neg(o) => {
            let t = operand_type(o, schema)?;
            if !t.is_numeric() {
                bail!("`-{o}` negates {t}, but only a number can be negated");
            }
            Ok(t)
        }
        Operand::Arith(a, op, b) => {
            let (at, bt) = (operand_type(a, schema)?, operand_type(b, schema)?);
            // `+` also concatenates strings, as CEL's does. The head needs it
            // (§4e: `site.url + url` is a canonical URL) and nothing else in
            // the language wanted it, which is why it arrived late.
            if at.scalar() == Type::Str && bt.scalar() == Type::Str && *op == ArithOp::Add {
                return Ok(Type::Str);
            }
            for (side, t) in [(a, at), (b, bt)] {
                if !t.is_numeric() {
                    bail!("`{side}` is {t}, but `{}` needs numbers", op.as_str());
                }
            }
            Ok(if at == Type::Double || bt == Type::Double {
                Type::Double
            } else {
                Type::Int
            })
        }
        Operand::Map(entries) => {
            let mut seen_keys = Vec::new();
            for (k, v) in entries {
                let kt = operand_type(k, schema)?;
                match kt.scalar() {
                    Type::Int | Type::Bool | Type::Str => {}
                    _ => bail!("map keys must be int, bool, or string, but `{k}` is {kt}"),
                }
                // Duplicate literal keys are a load error (CEL). Non-literal
                // keys are checked at eval.
                if let Operand::Lit(lit) = k {
                    let key = lit.value();
                    if seen_keys.iter().any(|s| s == &key) {
                        bail!("duplicate map key `{k}`");
                    }
                    seen_keys.push(key);
                }
                let _ = operand_type(v, schema)?;
            }
            Ok(Type::Map)
        }
        Operand::Index(base, idx) => {
            let bt = operand_type(base, schema)?;
            if bt.scalar() != Type::List {
                bail!("`{base}` is {bt}, but only a list can be indexed");
            }
            let it = operand_type(idx, schema)?;
            if it.scalar() != Type::Int {
                bail!("list index `{idx}` is {it}, but must be int");
            }
            // Element type is not tracked (string lists and Content lists
            // share `list`); the field's declared return type checks the use.
            Ok(Type::Any)
        }
        Operand::Cond(cond, then, otherwise) => {
            check(cond, schema)?;
            let tt = operand_type(then, schema)?;
            let et = operand_type(otherwise, schema)?;
            match (tt.scalar(), et.scalar()) {
                (Type::Any, t) | (t, Type::Any) => Ok(t),
                (a, b) if a == b => Ok(tt),
                _ => bail!(
                    "conditional branches must match, but `{then}` is {tt} and `{otherwise}` is {et}"
                ),
            }
        }
    }
}

/// The two sides of a comparison must be the same kind — both numbers (Int
/// and Double mix), or the identical scalar type. A list on either side is
/// the `in`-not-`==` mistake; ordering a bool is meaningless.
fn check_cmp(l: &Operand, op: Op, r: &Operand, schema: &Schema) -> Result<()> {
    let (lt, rt) = (operand_type(l, schema)?, operand_type(r, schema)?);
    if lt.scalar() == Type::List || rt.scalar() == Type::List {
        let (list, other) = if lt.scalar() == Type::List {
            (l, r)
        } else {
            (r, l)
        };
        bail!("`{list}` is a list; use `{other} in {list}` instead of a comparison");
    }
    let ok = lt.scalar() == rt.scalar() || (lt.is_numeric() && rt.is_numeric());
    if !ok {
        // Keep the original single-literal wording when the right side is a
        // literal, which is what the corpus and the tests write.
        if let Operand::Lit(_) = r {
            bail!("`{l}` is {lt}, but it is compared to a {rt} literal");
        }
        bail!("`{l}` is {lt}, but `{r}` is {rt} — a comparison needs matching types");
    }
    if op.is_ordering() && lt.scalar() == Type::Bool {
        bail!("`{l}` is bool; ordering comparisons are not meaningful");
    }
    // Both orders, because `"post" == kind` says exactly what `kind == "post"`
    // says and a checker that only knows one of them is a checker a config can
    // step around.
    check_domain(l, lt, op, r)?;
    check_domain(r, rt, op, l)
}

/// A closed-domain column compared against a literal outside its domain
///.
///
/// This is the same knife `resolve` takes to an unknown FIELD, one level down:
/// `kind == "posts"` type-checks perfectly — a string column against a string
/// literal — and then matches nothing, for as long as the config lives. The
/// domain is what turns "false forever" into a load error, and it can only be
/// declared by a column whose values the engine enumerates.
///
/// Non-literal right-hand sides are not checked and cannot be: `kind == view`
/// compares two columns, and neither one is a value.
///
/// The tail says what the constant is, and it is **not always `false`**
/// (batch review I-A): `kind != "posts"` is the same mistake — the
/// author meant to exclude something, and excluded nothing — but it can only
/// ever be TRUE, and a message that said "false" would send its reader looking
/// for a predicate that never fires when theirs always does. An ordering
/// comparison against an out-of-domain value is not constant at all (`kind >
/// "pages"` splits the domain), so it gets the honest neutral sentence rather
/// than an invented one; it is still an error, because the literal is still a
/// value the column can never hold.
fn check_domain(col: &Operand, col_ty: Type, op: Op, other: &Operand) -> Result<()> {
    let Some(domain) = col_ty.domain() else {
        return Ok(());
    };
    let Operand::Lit(Lit::Str(v)) = other else {
        return Ok(());
    };
    if domain.contains(&v.as_str()) {
        return Ok(());
    }
    let hint = domain
        .iter()
        .map(|k| (levenshtein(v, k), *k))
        .filter(|(d, _)| *d <= 2)
        .min_by_key(|(d, _)| *d)
        .map(|(_, k)| format!(" (did you mean `{k}`?)"))
        .unwrap_or_default();
    let tail = match op {
        Op::Eq => "the comparison can only ever be false",
        Op::Ne => "the comparison can only ever be true",
        _ => "the comparison orders against a value the column never holds",
    };
    bail!(
        "`{col}` is never {v:?}{hint} — {tail}\n  {col} is one of: {}",
        domain.join(", ")
    )
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
        Expr::Cmp(l, op, r) => check_cmp(l, *op, r, schema),
        Expr::In(l, r) => {
            let rt = operand_type(r, schema)?;
            if rt.scalar() != Type::List {
                bail!("`in` needs a list on the right, but `{r}` is {rt}");
            }
            // A string in a list (`"rust" in tags`) or a row in a relation's
            // finished list (`candidate in earlier`) — both spelled the same,
            // because a row reaches the environment as its URL (a Str).
            let lt = operand_type(l, schema)?;
            if lt.scalar() != Type::Str {
                bail!("`in` needs a string on the left, but `{l}` is {lt}");
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

    /// Every field name the filter reads. Relations use this to find which
    /// other relations a `where` depends on (`!(candidate in earlier)` names
    /// `earlier`), so they can evaluate in dependency order.
    pub fn referenced_fields(&self) -> Vec<String> {
        let mut out = Vec::new();
        collect_fields_expr(&self.ast, &mut out);
        out
    }

    /// Evaluate against a row, calling no score function (`NoCtx`). Every
    /// view filter takes this path.
    pub fn eval(&self, row: &impl Row) -> bool {
        eval(&self.ast, row, &NoCtx)
    }

    /// Evaluate with a `Ctx`, so a `where` that calls a score function
    /// (rare, but nothing forbids it) resolves. Relations use this.
    pub fn eval_ctx(&self, row: &impl Row, ctx: &dyn Ctx) -> bool {
        eval(&self.ast, row, ctx)
    }
}

/// A `rank` expression (§6g): a number per (self, candidate) pair, bigger
/// wins. Parsed and type-checked exactly like a filter, but the whole
/// expression must be numeric rather than boolean.
#[derive(Debug, Clone)]
pub struct Rank {
    op: Operand,
}

impl Rank {
    pub fn parse(src: &str, schema: &Schema) -> Result<Self> {
        let toks = lex(src)?;
        if toks.is_empty() {
            bail!("a rank expression cannot be empty");
        }
        let mut p = Parser { toks, pos: 0 };
        let op = p.parse_arith()?;
        if p.pos != p.toks.len() {
            bail!("trailing tokens after a complete expression");
        }
        let t = operand_type(&op, schema)?;
        if !t.is_numeric() {
            bail!("a rank must be a number, but `{op}` is {t}");
        }
        Ok(Rank { op })
    }

    /// The score, or `None` when an input was missing (an undated row in
    /// `year_gap`, a row with no vector) — an unrankable pair, dropped before
    /// the window rather than sorted to an arbitrary end.
    pub fn eval(&self, row: &impl Row, ctx: &dyn Ctx) -> Option<f64> {
        operand_value(&self.op, row, ctx).as_f64()
    }

    /// Every field name the rank reads — same purpose as `Filter`'s.
    pub fn referenced_fields(&self) -> Vec<String> {
        let mut out = Vec::new();
        collect_fields_operand(&self.op, &mut out);
        out
    }
}

/// A `text` expression: a string per row, for the places where the answer is a
/// STRING rather than a yes/no or a score — `[html.head.meta]` (§4e) is the
/// first, and the reason this exists.
///
/// Two shapes, and the first is why a conditional lives here at all:
///
/// ```text
/// noindex ? "noindex,follow" : ""     # CEL's own conditional
/// description                         # a bare operand
/// ```
///
/// §5d's no-control-flow rule governs TEMPLATES — a fragment wanting an `if`
/// means a missing fact. This is the expression surface, which is exactly
/// where a conditional is legitimate, and "which string does this meta take"
/// has no fact-shaped spelling. Spelled as CEL's `a ? b : c` rather than an
/// `if_else()` function so §5f's "grammatically valid CEL" contract costs
/// nothing to keep.
///
/// An empty result means **absent** — the same rule as §5e's "an empty part
/// deletes its element", one layer up: an empty meta value emits no tag.
#[derive(Debug, Clone)]
pub struct Text {
    cond: Option<Expr>,
    then: Operand,
    /// `None` for the bare-operand form, where `then` is the whole answer.
    otherwise: Option<Operand>,
}

impl Text {
    pub fn parse(src: &str, schema: &Schema) -> Result<Self> {
        let toks = lex(src)?;
        if toks.is_empty() {
            bail!("a text expression cannot be empty");
        }
        // Try the conditional first: its condition is a full boolean
        // expression, so it has to be parsed as one before we know which
        // shape this is.
        let mut p = Parser {
            toks: toks.clone(),
            pos: 0,
        };
        if let Ok(cond) = p.parse_or() {
            if matches!(p.toks.get(p.pos), Some(Tok::Question)) {
                p.pos += 1;
                let then = p.parse_arith()?;
                if !matches!(p.toks.get(p.pos), Some(Tok::Colon)) {
                    bail!("a conditional needs `:` — `cond ? \"a\" : \"b\"`");
                }
                p.pos += 1;
                let otherwise = p.parse_arith()?;
                if p.pos != p.toks.len() {
                    bail!("trailing tokens after a complete expression");
                }
                check(&cond, schema)?;
                for op in [&then, &otherwise] {
                    let t = operand_type(op, schema)?;
                    if t.scalar() != Type::Str && t.scalar() != Type::Any {
                        bail!("a conditional's branches must be strings, but `{op}` is {t}");
                    }
                }
                return Ok(Text {
                    cond: Some(cond),
                    then,
                    otherwise: Some(otherwise),
                });
            }
        }
        // Not a conditional: the whole thing is one string-valued operand.
        let mut p = Parser { toks, pos: 0 };
        let then = p.parse_arith()?;
        if p.pos != p.toks.len() {
            bail!("trailing tokens after a complete expression");
        }
        let t = operand_type(&then, schema)?;
        if t.scalar() != Type::Str && t.scalar() != Type::Any {
            bail!("a text expression must be a string, but `{then}` is {t}");
        }
        Ok(Text {
            cond: None,
            then,
            otherwise: None,
        })
    }

    /// The string, or `""` when the value is Null — an absent field says
    /// nothing rather than printing "null".
    pub fn eval(&self, row: &impl Row) -> String {
        let op = match (&self.cond, &self.otherwise) {
            (Some(c), Some(other)) => {
                if eval(c, row, &NoCtx) {
                    &self.then
                } else {
                    other
                }
            }
            _ => &self.then,
        };
        match operand_value(op, row, &NoCtx) {
            Value::Str(s) => s,
            Value::Int(i) => i.to_string(),
            Value::Double(d) => d.to_string(),
            Value::Bool(b) => b.to_string(),
            // Null and List both say nothing rather than printing a shape.
            _ => String::new(),
        }
    }

    pub fn referenced_fields(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(c) = &self.cond {
            collect_fields_expr(c, &mut out);
        }
        collect_fields_operand(&self.then, &mut out);
        if let Some(o) = &self.otherwise {
            collect_fields_operand(o, &mut out);
        }
        out
    }
}

/// A computed-field expression (§5f / §6d): an operand over an environment
/// that binds `content`. Return type is checked at parse (`Content` or
/// `Outline`).
#[derive(Debug, Clone)]
pub struct FieldExpr {
    op: Operand,
    returns: Type,
}

impl FieldExpr {
    /// Parse and infer the return type from the row schema. The type is a
    /// property of the expression, not of the field's name — which is what
    /// lets `[schema.fields]` carry any name the site cares to compute (§5f).
    pub fn infer(src: &str, schema: &Schema) -> Result<Self> {
        let toks = lex(src)?;
        if toks.is_empty() {
            bail!("a field expression cannot be empty");
        }
        let mut p = Parser { toks, pos: 0 };
        let op = parse_field_operand(&mut p)?;
        if p.pos != p.toks.len() {
            bail!("trailing tokens after a complete expression");
        }
        let returns = operand_type(&op, schema)?.scalar();
        Ok(FieldExpr { op, returns })
    }

    /// Parse, then require the inferred type to be `want`. For the engine's
    /// own fixed-type field uses — a card `summary` is content, a widget
    /// argument is a string — where the surrounding code knows the shape it
    /// needs. The open `[schema.fields]` bag uses [`infer`] instead.
    pub fn parse(src: &str, schema: &Schema, want: Type) -> Result<Self> {
        let e = Self::infer(src, schema)?;
        if e.returns != Type::Any && e.returns != want.scalar() {
            bail!(
                "a field expression must be {want}, but `{}` is {}",
                e.op,
                e.returns
            );
        }
        Ok(FieldExpr {
            op: e.op,
            returns: want,
        })
    }

    pub fn returns(&self) -> Type {
        self.returns
    }

    pub fn eval(&self, row: &impl Row) -> Value {
        operand_value(&self.op, row, &NoCtx)
    }

    pub fn referenced_fields(&self) -> Vec<String> {
        let mut out = Vec::new();
        collect_fields_operand(&self.op, &mut out);
        out
    }
}

/// `cond ? then : else`, right-associative on the else branch; else a bare arith operand.
fn parse_field_operand(p: &mut Parser) -> Result<Operand> {
    let start = p.pos;
    if let Ok(cond) = p.parse_or() {
        if p.eat(&Tok::Question) {
            let then = p.parse_arith()?;
            if !p.eat(&Tok::Colon) {
                bail!("a conditional needs `:` — `cond ? a : b`");
            }
            let otherwise = parse_field_operand(p)?;
            return Ok(Operand::Cond(
                Box::new(cond),
                Box::new(then),
                Box::new(otherwise),
            ));
        }
    }
    p.pos = start;
    p.parse_arith()
}

/// Schema for `fields.NAME` expressions: binds `content`. Row fields come
/// from the site's `[schema]` (via `Config::field_expr_schema`).
pub fn field_schema() -> Schema {
    let mut s = Schema::new();
    s.insert("content", Type::Content);
    s
}

fn collect_fields_expr(e: &Expr, out: &mut Vec<String>) {
    match e {
        Expr::True => {}
        Expr::Truthy(o) => collect_fields_operand(o, out),
        Expr::Not(x) => collect_fields_expr(x, out),
        Expr::And(a, b) | Expr::Or(a, b) => {
            collect_fields_expr(a, out);
            collect_fields_expr(b, out);
        }
        Expr::Cmp(a, _, b) | Expr::In(a, b) => {
            collect_fields_operand(a, out);
            collect_fields_operand(b, out);
        }
    }
}

fn collect_fields_operand(o: &Operand, out: &mut Vec<String>) {
    match o {
        Operand::Field(name) => out.push(name.clone()),
        Operand::Lit(_) => {}
        Operand::Neg(x) => collect_fields_operand(x, out),
        Operand::Arith(a, _, b) => {
            collect_fields_operand(a, out);
            collect_fields_operand(b, out);
        }
        Operand::Call(_, args, _) => {
            for a in args {
                collect_fields_operand(a, out);
            }
        }
        Operand::Map(entries) => {
            for (k, v) in entries {
                collect_fields_operand(k, out);
                collect_fields_operand(v, out);
            }
        }
        Operand::Index(base, idx) => {
            collect_fields_operand(base, out);
            collect_fields_operand(idx, out);
        }
        Operand::Cond(cond, then, otherwise) => {
            collect_fields_expr(cond, out);
            collect_fields_operand(then, out);
            collect_fields_operand(otherwise, out);
        }
    }
}

fn cmp_values(a: &Value, op: Op, b: &Value) -> bool {
    use std::cmp::Ordering;
    // Numbers compare across Int/Double; everything else compares within its
    // own type.
    let ord = match (a, b) {
        (Value::Null, _) | (_, Value::Null) => {
            // A null compares equal to nothing and orders below everything.
            return matches!(op, Op::Ne);
        }
        (Value::Str(x), Value::Str(y)) => x.partial_cmp(y),
        (Value::Bool(x), Value::Bool(y)) => x.partial_cmp(y),
        (Value::List(x), Value::List(y)) => Some(cmp_str_lists(x, y)),
        _ => match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => x.partial_cmp(&y),
            _ => None,
        },
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
fn operand_value(o: &Operand, row: &impl Row, ctx: &dyn Ctx) -> Value {
    match o {
        Operand::Field(name) => row.field(name),
        Operand::Lit(lit) => lit.value(),
        Operand::Call(func, args, prep) => {
            let vals: Vec<Value> = args.iter().map(|a| operand_value(a, row, ctx)).collect();
            (func.eval)(prep, &vals, ctx)
        }
        // A missing input propagates as Null: `-year_gap(...)` on an undated
        // pair is unrankable, not zero.
        Operand::Neg(o) => match operand_value(o, row, ctx).as_f64() {
            Some(x) => Value::Double(-x),
            None => Value::Null,
        },
        Operand::Arith(a, op, b) => {
            let (l, r) = (operand_value(a, row, ctx), operand_value(b, row, ctx));
            // String `+` concatenates (checked at parse time). A Null side
            // reads as empty rather than poisoning the whole expression:
            // `date + "T00:00:00+00:00"` on an undated row is guarded by a
            // conditional, and a bare concat should still say something.
            if let (Value::Str(x), Value::Str(y)) = (&l, &r) {
                return Value::Str(format!("{x}{y}"));
            }
            match (l.as_f64(), r.as_f64()) {
                (Some(x), Some(y)) => Value::Double(op.apply(x, y)),
                _ => Value::Null,
            }
        }
        Operand::Map(entries) => {
            let mut out = Vec::with_capacity(entries.len());
            for (k, v) in entries {
                let key = operand_value(k, row, ctx);
                if !matches!(key, Value::Str(_) | Value::Int(_) | Value::Bool(_)) {
                    return Value::Null;
                }
                if out.iter().any(|(s, _)| values_eq(s, &key)) {
                    return Value::Null;
                }
                out.push((key, operand_value(v, row, ctx)));
            }
            Value::Map(out)
        }
        Operand::Index(base, idx) => {
            let list = operand_value(base, row, ctx);
            let i = match operand_value(idx, row, ctx) {
                Value::Int(n) if n >= 0 => n as usize,
                _ => return Value::Null,
            };
            match list {
                Value::List(items) => items.get(i).cloned().unwrap_or(Value::Null),
                _ => Value::Null,
            }
        }
        Operand::Cond(cond, then, otherwise) => {
            let branch = if eval(cond, row, ctx) {
                then
            } else {
                otherwise
            };
            operand_value(branch, row, ctx)
        }
    }
}

fn values_eq(a: &Value, b: &Value) -> bool {
    a == b
}

fn eval(e: &Expr, row: &impl Row, ctx: &dyn Ctx) -> bool {
    match e {
        Expr::True => true,
        Expr::Truthy(o) => operand_value(o, row, ctx).truthy(),
        Expr::Not(x) => !eval(x, row, ctx),
        Expr::And(a, b) => eval(a, row, ctx) && eval(b, row, ctx),
        Expr::Or(a, b) => eval(a, row, ctx) || eval(b, row, ctx),
        Expr::Cmp(l, op, r) => cmp_values(
            &operand_value(l, row, ctx),
            *op,
            &operand_value(r, row, ctx),
        ),
        Expr::In(l, r) => match (operand_value(l, row, ctx), operand_value(r, row, ctx)) {
            (Value::Str(s), Value::List(items)) => {
                items.iter().any(|v| matches!(v, Value::Str(t) if t == &s))
            }
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
                "tags" => Value::str_list(self.tags.clone()),
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
        // A closed-domain column beside an open one, so every assertion about
        // the domain has a `title` to compare itself against.
        s.insert("kind", Type::Enum(&["post", "page", "view"]));
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

    /// `glob` is typed like every other registered function (§5f), which is
    /// what a view's retired `match` key bought by dissolving into `where`
    ///: the path scope is now checked by the same pass, against
    /// the same vocabulary, and both ways of getting it wrong say so.
    #[test]
    fn glob_is_typed_like_any_other_function() {
        // Wrong TYPE: the error names both what the parameter wants and what
        // the argument is.
        let e = err(r#"glob(year, "recipes/**")"#);
        assert!(
            e.contains("`glob` argument 1 is string, but `year` is int"),
            "{e}"
        );
        // Wrong NAME: the field resolver answers with the knowns, so an
        // object view's narrow vocabulary is legible at the point of failure.
        let e = err(r#"glob(paht, "recipes/**")"#);
        assert!(e.contains("unknown field `paht`"), "{e}");
        assert!(e.contains("did you mean `path`"), "{e}");
        assert!(
            e.contains("known fields: description, draft, hidden, kind, path, tags, title, year"),
            "{e}"
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

    /// The same knife one level down: `kind == "posts"` names a
    /// real column at the right type and then matches nothing forever.
    ///
    /// Mutation: delete either `check_domain` call in `check_cmp` and the
    /// first assertion fails — the expression parses, and `Filter::eval`
    /// returns false for every row a route pool could ever hold.
    #[test]
    fn a_value_outside_a_closed_domain_is_an_error_with_the_knowns() {
        let e = |s: &str| Filter::parse(s, &schema()).unwrap_err().to_string();
        let msg = e(r#"kind == "posts""#);
        assert!(msg.contains(r#"`kind` is never "posts""#), "{msg}");
        assert!(msg.contains("did you mean `post`"), "{msg}");
        assert!(msg.contains("kind is one of: post, page, view"), "{msg}");
        // Both orders, and every operator: a domain is a fact about the
        // column, not about which side of the `==` it was written on.
        assert!(e(r#""posts" == kind"#).contains("is never \"posts\""));
        assert!(e(r#"kind != "pages""#).contains("is never \"pages\""));
    }

    /// The tail names the constant the comparison became, and which constant
    /// that is depends on the OPERATOR (batch review I-A).
    ///
    /// `kind != "posts"` is the same authoring mistake as `kind == "posts"` —
    /// an exclusion that excludes nothing — but it is true forever, not false
    /// forever, and the old message said "false" for both. A reader who trusts
    /// it goes looking for a predicate that never fires when theirs always
    /// does, which is the opposite end of the same haystack. Ordering gets
    /// neither word: `kind > "pages"` against a value outside the domain is
    /// not constant at all, so the sentence says what IS true.
    ///
    /// Mutation: collapse the `match op` back to a single "false" arm and the
    /// `!=` and ordering assertions fail.
    #[test]
    fn the_constant_the_message_names_follows_the_operator() {
        let e = |s: &str| Filter::parse(s, &schema()).unwrap_err().to_string();
        assert!(
            e(r#"kind == "posts""#).contains("can only ever be false"),
            "{}",
            e(r#"kind == "posts""#)
        );
        let ne = e(r#"kind != "posts""#);
        assert!(ne.contains("can only ever be true"), "{ne}");
        assert!(!ne.contains("can only ever be false"), "{ne}");
        // The hint rides along whatever the operator is — it is about the
        // literal, not about the comparison.
        assert!(ne.contains("did you mean `post`"), "{ne}");
        let gt = e(r#"kind > "posts""#);
        assert!(gt.contains("never holds"), "{gt}");
        assert!(!gt.contains("can only ever be"), "{gt}");
        // Both orders, still: the domain is a fact about the column.
        assert!(
            e(r#""posts" != kind"#).contains("can only ever be true"),
            "{}",
            e(r#""posts" != kind"#)
        );
    }

    /// The domain buys the check and nothing else. Every value IN it parses,
    /// an open string column is untouched, and comparing two columns — where
    /// neither side is a value — is not something a domain can rule on.
    #[test]
    fn a_closed_domain_is_otherwise_an_ordinary_string_column() {
        ok(r#"kind == "post""#);
        ok(r#"kind == "view" || kind == "page""#);
        ok(r#"kind == title"#);
        ok(r#"kind + "!" == "post!""#);
        // The control: `title` has no domain, so an arbitrary literal is the
        // author's business.
        ok(r#"title == "posts""#);
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

    // ---- §4e: text expressions, for the places the answer is a string ----

    /// CEL's own conditional, which is the whole reason `Text` exists: the
    /// engine stopped knowing that a robots meta says "noindex".
    #[test]
    fn a_conditional_picks_a_branch() {
        let t = Text::parse(r#"draft ? "noindex,follow" : """#, &schema()).unwrap();
        assert_eq!(t.eval(&TestRow::default()), "");
        let drafted = TestRow {
            draft: true,
            ..Default::default()
        };
        assert_eq!(t.eval(&drafted), "noindex,follow");
    }

    /// The bare form: a field is a whole expression.
    #[test]
    fn a_bare_operand_is_a_text_expression() {
        let t = Text::parse("title", &schema()).unwrap();
        assert_eq!(t.eval(&TestRow::default()), "Hello");
        // An absent value says nothing rather than printing "null" — which is
        // what makes "empty means the tag is not emitted" total.
        let t = Text::parse("description", &schema()).unwrap();
        assert_eq!(t.eval(&TestRow::default()), "");
    }

    /// Type-checked at load like everything else (§5f).
    #[test]
    fn a_text_expression_is_checked_at_load() {
        let e = |s: &str| Text::parse(s, &schema()).unwrap_err().to_string();
        assert!(e("year").contains("must be a string"), "{}", e("year"));
        assert!(e("draft ? 1 : 2").contains("branches must be strings"));
        assert!(e("draft ? \"a\"").contains("needs `:`"));
        assert!(e("nope").contains("unknown field"));
        assert!(e("").contains("cannot be empty"));
    }

    /// String `+` concatenates, as CEL's does. The head needed it (§4e:
    /// `site.url + url`) and nothing else in the language had.
    #[test]
    fn plus_concatenates_strings() {
        let t = Text::parse(r#"title + "!" + path"#, &schema()).unwrap();
        assert_eq!(
            t.eval(&TestRow::default()),
            "Hello!recipes/pasta/carbonara.md"
        );
        // Still numbers-only for the other operators, and still typed.
        let e = Text::parse(r#"title - "x""#, &schema())
            .unwrap_err()
            .to_string();
        assert!(e.contains("needs numbers"), "{e}");
    }

    #[test]
    fn a_conditional_reads_both_sides() {
        let t = Text::parse(r#"draft ? title : path"#, &schema()).unwrap();
        let mut f = t.referenced_fields();
        f.sort();
        assert_eq!(f, ["draft", "path", "title"]);
    }

    #[test]
    fn empty_filter_matches_everything() {
        let r = TestRow::default();
        assert!(Filter::parse("", &schema()).unwrap().eval(&r));
        assert!(Filter::always().eval(&r));
    }

    // ---- §6g: arithmetic, rank, name membership, the two-row environment ----

    /// A two-row environment (§6g): `self.*`/`candidate.*` are ordinary
    /// schema keys (the lexer already treats `.` as part of an identifier),
    /// plus `candidate` as the row's URL and each relation name as a list.
    struct PairRow {
        self_year: i64,
        cand_year: i64,
        cand_url: String,
        earlier: Vec<String>,
    }

    impl Row for PairRow {
        fn field(&self, name: &str) -> Value {
            match name {
                "self.year" => Value::Int(self.self_year),
                "candidate.year" => Value::Int(self.cand_year),
                "self" | "self.url" => Value::Str("/self/".into()),
                "candidate" => Value::Str(self.cand_url.clone()),
                "earlier" => Value::str_list(self.earlier.clone()),
                _ => Value::Null,
            }
        }
    }

    fn pair_schema() -> Schema {
        let mut s = Schema::new();
        s.insert("self.year", Type::Int);
        s.insert("candidate.year", Type::Int);
        s.insert("self", Type::Str);
        s.insert("self.url", Type::Str);
        s.insert("candidate", Type::Str);
        s.insert("earlier", Type::List);
        s
    }

    /// A ctx that knows one similarity and one year-gap, keyed loosely — just
    /// enough to prove the plumbing carries data into a score function.
    struct FakeCtx;
    impl Ctx for FakeCtx {
        fn similarity(&self, a: &str, b: &str) -> Option<f64> {
            (a != b).then_some(0.8)
        }
        fn year_gap(&self, _: &str, _: &str) -> Option<f64> {
            Some(3.0)
        }
    }

    #[test]
    fn floats_lex_and_arithmetic_types() {
        // `0.01` is a float; `year` stays int; the sum is a double.
        let s = pair_schema();
        assert!(Rank::parse("self.year + 0.01", &s).is_ok());
        assert!(Rank::parse("candidate.year", &s).is_ok());
        let e = Rank::parse("candidate", &s).unwrap_err().to_string();
        assert!(e.contains("must be a number"), "{e}");
    }

    #[test]
    fn rank_evaluates_with_a_ctx() {
        let r = PairRow {
            self_year: 2020,
            cand_year: 2017,
            cand_url: "/b/".into(),
            earlier: vec![],
        };
        // grack.com's shape: similarity minus a small year penalty.
        let rk = Rank::parse(
            "embedding_similarity(candidate, candidate) - 0.01 * year_gap(self.dummy, candidate)",
            &{
                let mut s = pair_schema();
                s.insert("self.dummy", Type::Str);
                s
            },
        );
        // `candidate == candidate` similarity is None in FakeCtx, so the pair
        // is unrankable — the whole expression is None, not a partial score.
        assert_eq!(rk.unwrap().eval(&r, &FakeCtx), None);

        // A distinct pair ranks: 0.8 - 0.01*3 = 0.77.
        let mut s = pair_schema();
        s.insert("self.url", Type::Str);
        let rk = Rank::parse(
            "embedding_similarity(self.url, candidate) - 0.01 * year_gap(self.url, candidate)",
            &s,
        )
        .unwrap();
        let score = rk.eval(&r, &FakeCtx).unwrap();
        assert!((score - 0.77).abs() < 1e-9, "got {score}");
    }

    #[test]
    fn a_distance_function_wears_a_minus_sign() {
        let mut s = Schema::new();
        s.insert("a", Type::Str);
        s.insert("b", Type::Str);
        let rk = Rank::parse("-levenshtein(a, b)", &s).unwrap();
        struct R;
        impl Row for R {
            fn field(&self, n: &str) -> Value {
                Value::Str(if n == "a" { "kitten" } else { "sitting" }.into())
            }
        }
        // edit distance kitten→sitting is 3, negated so nearer ranks higher.
        assert_eq!(rk.eval(&R, &NoCtx), Some(-3.0));
    }

    #[test]
    fn name_membership_is_str_in_list() {
        // `candidate in earlier` — the row's URL against a relation's list.
        let f = Filter::parse("!(candidate in earlier)", &pair_schema()).unwrap();
        let shown = PairRow {
            self_year: 0,
            cand_year: 0,
            cand_url: "/a/".into(),
            earlier: vec!["/a/".into(), "/b/".into()],
        };
        assert!(!f.eval(&shown), "candidate already in earlier is excluded");
        let fresh = PairRow {
            cand_url: "/c/".into(),
            ..shown
        };
        assert!(f.eval(&fresh));
    }

    #[test]
    fn comparison_between_two_fields() {
        // `candidate.year < self.year` — the earlier/later shape, no literal.
        let f = Filter::parse("candidate.year < self.year", &pair_schema()).unwrap();
        assert!(f.eval(&PairRow {
            self_year: 2020,
            cand_year: 2017,
            cand_url: "/x/".into(),
            earlier: vec![],
        }));
        assert!(!f.eval(&PairRow {
            self_year: 2020,
            cand_year: 2020,
            cand_url: "/x/".into(),
            earlier: vec![],
        }));
    }

    #[test]
    fn int_and_double_compare_and_mix() {
        let mut s = Schema::new();
        s.insert("n", Type::Int);
        struct R(i64);
        impl Row for R {
            fn field(&self, _: &str) -> Value {
                Value::Int(self.0)
            }
        }
        assert!(Filter::parse("n > 0.5", &s).unwrap().eval(&R(1)));
        assert!(!Filter::parse("n > 0.5", &s).unwrap().eval(&R(0)));
    }

    #[test]
    fn rank_rejects_a_non_numeric() {
        let mut s = Schema::new();
        s.insert("title", Type::Str);
        let e = Rank::parse("title", &s).unwrap_err().to_string();
        assert!(e.contains("must be a number"), "{e}");
        let e = Rank::parse("title + 1", &s).unwrap_err().to_string();
        assert!(e.contains("needs numbers"), "{e}");
    }

    /// The corner the review flagged: a parenthesised operand on the left of a
    /// comparison. Both spellings now give an informative error, not the old
    /// misleading "trailing tokens".
    #[test]
    fn parenthesised_left_of_a_comparison_errors_clearly() {
        let mut s = Schema::new();
        s.insert("a", Type::Int);
        s.insert("b", Type::Int);
        s.insert("d", Type::Bool);
        // Arithmetic group: fails as "not a condition" while parsing the group.
        let e = Filter::parse("(a + b) > 3", &s).unwrap_err().to_string();
        assert!(e.contains("not a condition"), "{e}");
        assert!(!e.contains("trailing tokens"), "{e}");
        // Boolean group then a comparison: the dedicated not-supported error.
        let e = Filter::parse("(d) > 3", &s).unwrap_err().to_string();
        assert!(e.contains("not supported yet"), "{e}");
    }

    #[test]
    fn search_similarity_is_no_longer_registered() {
        // Unwired until it has an implementation — a config naming it is a
        // load error, not a silently empty group.
        let mut s = Schema::new();
        s.insert("self.url", Type::Str);
        s.insert("candidate", Type::Str);
        let e = Rank::parse("search_similarity(self.url, candidate)", &s)
            .unwrap_err()
            .to_string();
        assert!(e.contains("unknown function `search_similarity`"), "{e}");
    }

    #[test]
    fn arithmetic_precedence_mul_over_add() {
        let mut s = Schema::new();
        s.insert("x", Type::Int);
        struct R;
        impl Row for R {
            fn field(&self, _: &str) -> Value {
                Value::Int(2)
            }
        }
        // 2 + 2*2 = 6, not 8 — `*` binds tighter.
        assert_eq!(
            Rank::parse("x + x * x", &s).unwrap().eval(&R, &NoCtx),
            Some(6.0)
        );
        // parens override.
        assert_eq!(
            Rank::parse("(x + x) * x", &s).unwrap().eval(&R, &NoCtx),
            Some(8.0)
        );
    }

    #[test]
    fn truncate_wrappers_compose_on_content() {
        let blocks = vec![
            "<p>aaaa aaaa</p>".into(),
            "<p>bbbb bbbb</p>".into(),
            "<p>cccc</p>".into(),
        ];
        let c = Content::new(blocks);
        let by_blocks = c.truncate_blocks(2).unwrap();
        assert_eq!(by_blocks.blocks().unwrap().len(), 2);
        assert!(by_blocks.truncated);

        let by_chars = c.truncate_chars(12).unwrap();
        assert_eq!(by_chars.blocks().unwrap().len(), 1);
        assert!(by_chars.truncated);

        let composed = c.truncate_blocks(4).unwrap().truncate_chars(12).unwrap();
        assert_eq!(composed.blocks().unwrap().len(), 1);
        assert!(composed.truncated);

        let whole = c.truncate_blocks(10).unwrap();
        assert!(!whole.truncated);
        assert_eq!(whole.html_string(), c.html_string());
    }

    #[test]
    fn field_expr_parses_truncate_chain() {
        let s = field_schema();
        let expr = FieldExpr::parse(
            "truncate_chars(truncate_blocks(content, 4), 700)",
            &s,
            Type::Content,
        )
        .unwrap();
        struct R(Content);
        impl Row for R {
            fn field(&self, name: &str) -> Value {
                match name {
                    "content" => Value::Content(self.0.clone()),
                    _ => Value::Null,
                }
            }
        }
        let row = R(Content::new(vec![
            "<p>one</p>".into(),
            "<p>two</p>".into(),
            "<p>three</p>".into(),
            "<p>four</p>".into(),
            "<p>five</p>".into(),
        ]));
        let Value::Content(out) = expr.eval(&row) else {
            panic!("expected content");
        };
        assert_eq!(out.blocks().unwrap().len(), 4);
        assert!(out.truncated);
    }

    #[test]
    fn field_expr_rejects_non_content() {
        let s = field_schema();
        let e = FieldExpr::parse("4", &s, Type::Content)
            .unwrap_err()
            .to_string();
        assert!(e.contains("must be content"), "{e}");
    }

    #[test]
    fn outline_builds_heading_tree() {
        let c = Content::new(vec![
            "<h2 id=\"a\">Alpha</h2>".into(),
            "<p>x</p>".into(),
            "<h3 id=\"b\">Beta</h3>".into(),
            "<h2 id=\"c\">Gamma</h2>".into(),
        ]);
        let s = field_schema();
        let expr = FieldExpr::parse("outline(content, 3)", &s, Type::Outline).unwrap();
        struct R(Content);
        impl Row for R {
            fn field(&self, name: &str) -> Value {
                match name {
                    "content" => Value::Content(self.0.clone()),
                    _ => Value::Null,
                }
            }
        }
        let Value::Outline(tree) = expr.eval(&R(c)) else {
            panic!("expected outline");
        };
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].label, "Alpha");
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].label, "Beta");
        assert_eq!(tree[1].label, "Gamma");
    }

    struct Empty;
    impl Row for Empty {
        fn field(&self, _: &str) -> Value {
            Value::Null
        }
    }

    #[test]
    fn map_literal_round_trips_through_to_json() {
        let s = Schema::new();
        let t = Text::parse(r#"to_json({"a": 1, "b": true, "c": "x"})"#, &s).unwrap();
        let json = t.eval(&Empty);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["a"], 1);
        assert_eq!(v["b"], true);
        assert_eq!(v["c"], "x");
    }

    #[test]
    fn empty_map_and_trailing_comma() {
        let s = Schema::new();
        assert_eq!(Text::parse("to_json({})", &s).unwrap().eval(&Empty), "{}");
        let t = Text::parse(r#"to_json({"a": 1,})"#, &s).unwrap();
        assert_eq!(t.eval(&Empty), r#"{"a":1}"#);
    }

    #[test]
    fn map_keys_may_be_int_or_bool() {
        let s = Schema::new();
        let t = Text::parse(r#"to_json({1: "one", true: "yes"})"#, &s).unwrap();
        let v: serde_json::Value = serde_json::from_str(&t.eval(&Empty)).unwrap();
        assert_eq!(v["1"], "one");
        assert_eq!(v["true"], "yes");
    }

    #[test]
    fn duplicate_map_key_is_a_load_error() {
        let s = Schema::new();
        let e = Text::parse(r#"to_json({"a": 1, "a": 2})"#, &s)
            .unwrap_err()
            .to_string();
        assert!(e.contains("duplicate map key"), "{e}");
    }

    #[test]
    fn map_key_must_be_int_bool_or_string() {
        let mut schema = Schema::new();
        schema.insert("tags", Type::List);
        let e = Text::parse(r#"to_json({tags: 1})"#, &schema)
            .unwrap_err()
            .to_string();
        assert!(e.contains("map keys must be"), "{e}");
    }

    #[test]
    fn content_kinds_and_coercion() {
        let s = Schema::new();
        assert_eq!(
            Text::parse(r#"kind(html("<p>x</p>"))"#, &s)
                .unwrap()
                .eval(&Empty),
            "html"
        );
        assert_eq!(
            Text::parse(r#"kind(as_text(html("<p>a b</p>")))"#, &s)
                .unwrap()
                .eval(&Empty),
            "text"
        );
        assert_eq!(
            Text::parse(r#"to_json(word_count(text("one two three")))"#, &s)
                .unwrap()
                .eval(&Empty),
            "3"
        );
        assert_eq!(
            Text::parse(r#"to_json(word_count(html("<p>one two</p>")))"#, &s)
                .unwrap()
                .eval(&Empty),
            "2"
        );
        assert_eq!(
            Text::parse(r#"to_json(word_count(markdown("one **two**")))"#, &s)
                .unwrap()
                .eval(&Empty),
            "2"
        );
        assert_eq!(
            Text::parse(r#"to_json(as_markdown(html("<p>x</p>")))"#, &s)
                .unwrap()
                .eval(&Empty),
            "null"
        );
        assert_eq!(
            Text::parse(r#"to_json(as_html(text("x")))"#, &s)
                .unwrap()
                .eval(&Empty),
            "null"
        );
        assert_eq!(
            Text::parse(r#"kind(as_html(markdown("hi")))"#, &s)
                .unwrap()
                .eval(&Empty),
            "html"
        );
    }

    #[test]
    fn content_coercion_typechecks_at_load() {
        let s = field_schema();
        FieldExpr::parse("as_html(content)", &s, Type::Content).unwrap();
        FieldExpr::parse("word_count(content)", &s, Type::Int).unwrap();
        let e = FieldExpr::parse("as_html(4)", &s, Type::Content)
            .unwrap_err()
            .to_string();
        assert!(e.contains("content"), "{e}");
    }

    #[test]
    fn links_and_images_extract_and_index() {
        let s = Schema::new();
        assert_eq!(
            Text::parse(
                r#"to_json(links(html('<p><a href="/a">x</a> and <a href="/b">y</a></p>')))"#,
                &s
            )
            .unwrap()
            .eval(&Empty),
            r#"["/a","/b"]"#
        );
        assert_eq!(
            Text::parse(
                r#"to_json(images(html('<img src="a.png" alt=""><img src="b.jpg">')))"#,
                &s
            )
            .unwrap()
            .eval(&Empty),
            r#"["a.png","b.jpg"]"#
        );
        assert_eq!(
            Text::parse(
                r#"links(html('<a href="/first">x</a><a href="/second">y</a>'))[0]"#,
                &s
            )
            .unwrap()
            .eval(&Empty),
            "/first"
        );
        assert_eq!(
            Text::parse(r#"to_json(images(html('<img src="only.png">'))[1])"#, &s)
                .unwrap()
                .eval(&Empty),
            "null"
        );
        assert_eq!(
            Text::parse(r#"to_json(links(text("no tags")))"#, &s)
                .unwrap()
                .eval(&Empty),
            "[]"
        );
        assert_eq!(
            Text::parse(r#"images(markdown("![hi](pic.png)"))[0]"#, &s)
                .unwrap()
                .eval(&Empty),
            "pic.png"
        );
        FieldExpr::parse("links(content)[0]", &field_schema(), Type::Str).unwrap();
    }

    #[test]
    fn hero_field_prefers_cover_then_image_then_body() {
        let mut s = field_schema();
        s.insert("cover", Type::Str);
        s.insert("image", Type::Str);
        let expr = FieldExpr::parse(
            "cover ? cover : image ? image : images(content)[0]",
            &s,
            Type::Str,
        )
        .unwrap();
        struct R {
            cover: &'static str,
            image: &'static str,
            content: Content,
        }
        impl Row for R {
            fn field(&self, name: &str) -> Value {
                match name {
                    "cover" => Value::Str(self.cover.into()),
                    "image" => Value::Str(self.image.into()),
                    "content" => Value::Content(self.content.clone()),
                    _ => Value::Null,
                }
            }
        }
        let body = Content::new(vec![r#"<p><img src="from-body.png"></p>"#.into()]);
        assert_eq!(
            expr.eval(&R {
                cover: "cover.png",
                image: "",
                content: body.clone(),
            }),
            Value::Str("cover.png".into())
        );
        assert_eq!(
            expr.eval(&R {
                cover: "",
                image: "image.png",
                content: body.clone(),
            }),
            Value::Str("image.png".into())
        );
        assert_eq!(
            expr.eval(&R {
                cover: "",
                image: "",
                content: body,
            }),
            Value::Str("from-body.png".into())
        );
    }

    #[test]
    fn html_drills_through_divs_and_text_chunks_paragraphs() {
        let c =
            Content::from_html_document(r#"<div><div><h1>T</h1><p>one</p></div><p>two</p></div>"#);
        let tags: Vec<_> = c.blocks().unwrap().iter().map(|b| b.tag.as_str()).collect();
        assert_eq!(tags, ["h1", "p", "p"]);

        let t = Content::text("alpha\n\nbeta gamma\n\n");
        let ps = t.filter_blocks(&["p"]);
        assert_eq!(ps.len(), 2);
        assert!(ps[0].html_string().contains("alpha"));
        assert!(ps[1].html_string().contains("beta gamma"));
    }

    #[test]
    fn filter_blocks_index_yields_first_paragraph() {
        let expr = FieldExpr::parse(
            r#"filter_blocks(content, "p")[0]"#,
            &field_schema(),
            Type::Content,
        )
        .unwrap();
        struct R(Content);
        impl Row for R {
            fn field(&self, name: &str) -> Value {
                match name {
                    "content" => Value::Content(self.0.clone()),
                    _ => Value::Null,
                }
            }
        }
        let row = R(Content::new(vec![
            "<h1>Title</h1>".into(),
            "<p>lede text</p>".into(),
            "<p>more</p>".into(),
        ]));
        let Value::Content(out) = expr.eval(&row) else {
            panic!("expected content");
        };
        assert_eq!(out.blocks().unwrap().len(), 1);
        assert_eq!(out.blocks().unwrap()[0].tag, "p");
        assert!(out.html_string().contains("lede text"));
        assert!(!out.html_string().contains("more"));
    }

    #[test]
    fn keep_blocks_and_filter_blocks_take_multiple_tags() {
        let c = Content::new(vec![
            "<h1>T</h1>".into(),
            "<p>one</p>".into(),
            "<ul><li>x</li></ul>".into(),
            "<h2>S</h2>".into(),
            "<p>two</p>".into(),
        ]);
        let kept = c.keep_blocks(&["p", "h1"]);
        let tags: Vec<_> = kept
            .blocks()
            .unwrap()
            .iter()
            .map(|b| b.tag.as_str())
            .collect();
        assert_eq!(tags, ["h1", "p", "p"]);
        assert!(kept.truncated);

        let expr = FieldExpr::parse(
            r#"keep_blocks(content, "p", "h1")"#,
            &field_schema(),
            Type::Content,
        )
        .unwrap();
        struct R(Content);
        impl Row for R {
            fn field(&self, name: &str) -> Value {
                match name {
                    "content" => Value::Content(self.0.clone()),
                    _ => Value::Null,
                }
            }
        }
        let Value::Content(out) = expr.eval(&R(c.clone())) else {
            panic!("expected content");
        };
        assert_eq!(out.blocks().unwrap().len(), 3);

        let list = FieldExpr::parse(
            r#"filter_blocks(content, "h1", "h2")"#,
            &field_schema(),
            Type::List,
        )
        .unwrap()
        .eval(&R(c));
        let Value::List(items) = list else {
            panic!("expected list");
        };
        assert_eq!(items.len(), 2);
    }
}

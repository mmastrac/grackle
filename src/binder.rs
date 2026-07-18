//! The fragment binder (§5e step 2): theme fragments are straight-line HTML
//! with typed holes; this module parses them once at load, validates every
//! name against the part schemas, and fills them from part maps at render.
//!
//! The whole hole algebra:
//!
//! 1. **`data-slot="name"`** — the element's content is replaced by the part.
//!    `Text` parts are escaped; `Html` parts are trusted. `Stream`/`Map` parts
//!    render a child fragment per item (the fragment for the child's kind, or
//!    the `data-fragment` override). The loop lives here; fragments stay
//!    straight-line.
//! 2. **An empty part deletes its element.** Absent part, empty text, empty
//!    stream: the element does not render. This one rule is every
//!    presence-conditional (§5d's "genuinely hard" case, dissolved).
//! 3. **`data-slot-attr="name"`** — the attribute `attr` is set from a `Text`
//!    part, escaped; an absent part omits the attribute. `<a>` with no `href`
//!    is the HTML-spec placeholder link, so "linked crumb vs inert tail" and
//!    "page number vs current page" are `a:not([href])` in theme CSS, not a
//!    branch anywhere.
//!
//! Facts (`Flag` parts) never fill holes: the root element of a rendered
//! fragment is stamped `data-kind="<kind>"` plus `data-<name>` per true flag,
//! and theme CSS selects on them (§5e: facts as attributes).
//!
//! **Everything is checked at load, not at render**: unknown slot, slot/type
//! mismatch, content slot on a void element, `data-fragment` naming a missing
//! or wrong-kind fragment — errors that name the file and line and list the
//! known names, the filter-language discipline (§5) applied to themes. The
//! parser is deliberately strict (well-formed, double-quoted attributes,
//! matching close tags): a malformed fragment is a build error, not something
//! to recover from. After validation, rendering is infallible.

// Wired into the build at §5e step 3 (theme directories); until then the
// binder is exercised by its tests only.
#![allow(dead_code)]

use anyhow::{bail, Result};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use crate::parts::{Part, PartMap, PartType};
use crate::render::esc;

// ------------------------------------------------------------------- model

#[derive(Debug)]
enum Node {
    /// Verbatim output: text, comments, doctype.
    Text(String),
    Element(Element),
}

#[derive(Debug)]
struct Element {
    tag: String,
    /// Literal attributes, in source order. Slot-filled attributes keep their
    /// position here as placeholders (`data-slot-href` renders as `href`).
    attrs: Vec<Attr>,
    /// `data-slot` — this element is a content hole.
    slot: Option<String>,
    /// `data-fragment` — override the child fragment for a stream/map slot.
    fragment: Option<String>,
    children: Vec<Node>,
    void: bool,
    line: usize,
}

#[derive(Debug)]
enum Attr {
    /// `name="value"` or bare `name`, emitted verbatim.
    Literal(String, Option<String>),
    /// `data-slot-<attr>="part"`: fill `attr` from a Text part, or omit.
    Slot(String, String),
}

/// One parsed fragment file, bound to the layout kind its filename names.
#[derive(Debug)]
pub struct Fragment {
    pub kind: String,
    nodes: Vec<Node>,
}

/// A theme's fragment set, keyed by kind. Loading validates every fragment
/// against the part schemas; after that, `render` cannot fail.
#[derive(Debug, Default)]
pub struct Fragments {
    map: BTreeMap<String, Fragment>,
}

impl Fragments {
    /// Load every `*.html` in a theme directory. The file stem names the kind
    /// it binds to (`summary.html` → the `summary` part schema).
    pub fn load_dir(dir: &Path) -> Result<Fragments> {
        let mut sources = Vec::new();
        let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "html") {
                let stem = p.file_stem().unwrap().to_string_lossy().to_string();
                let text = std::fs::read_to_string(&p)?;
                sources.push((stem, text, p.display().to_string()));
            }
        }
        Self::load(sources)
    }

    /// Load from `(kind, source, display-name)` triples — the testable core.
    /// Parse everything first, then validate: cross-fragment checks (a stream
    /// slot needs its child fragment) need the whole set present.
    pub fn load(sources: Vec<(String, String, String)>) -> Result<Fragments> {
        let mut f = Fragments::default();
        let mut files = Vec::new();
        for (kind, text, file) in &sources {
            if crate::parts::schema(kind).is_none() {
                bail!(
                    "{file}: fragment names no layout kind `{kind}` — kinds are: {}",
                    known_kinds()
                );
            }
            let nodes = Parser::new(text, file).parse_all()?;
            f.map.insert(kind.clone(), Fragment { kind: kind.clone(), nodes });
            files.push((kind.clone(), file.clone()));
        }
        for (kind, file) in &files {
            f.validate(&f.map[kind], file)?;
        }
        Ok(f)
    }

    pub fn get(&self, kind: &str) -> Option<&Fragment> {
        self.map.get(kind)
    }

    // ------------------------------------------------------------ validate

    fn validate(&self, frag: &Fragment, file: &str) -> Result<()> {
        self.validate_nodes(&frag.nodes, &frag.kind, file)
    }

    fn validate_nodes(&self, nodes: &[Node], kind: &str, file: &str) -> Result<()> {
        for n in nodes {
            let Node::Element(el) = n else { continue };
            if let Some(slot) = &el.slot {
                let Some(ty) = crate::parts::part_type(kind, slot) else {
                    bail!(
                        "{file}:{}: unknown slot `{slot}` on <{}> — `{kind}` has: {}",
                        el.line,
                        el.tag,
                        known_parts(kind)
                    );
                };
                match ty {
                    PartType::Flag => bail!(
                        "{file}:{}: `{slot}` is a fact — facts become data- attributes \
                         on the fragment root, they do not fill content",
                        el.line
                    ),
                    PartType::Text | PartType::Url | PartType::Html => {
                        if el.void {
                            bail!(
                                "{file}:{}: content slot `{slot}` on void element <{}>",
                                el.line,
                                el.tag
                            );
                        }
                        if let Some(f) = &el.fragment {
                            bail!(
                                "{file}:{}: `{slot}` is a scalar; data-fragment=\"{f}\" \
                                 only applies to streams",
                                el.line
                            );
                        }
                    }
                    PartType::Stream(child) | PartType::Map(child) => {
                        if el.void {
                            bail!(
                                "{file}:{}: content slot `{slot}` on void element <{}>",
                                el.line,
                                el.tag
                            );
                        }
                        // A theme without a fragment for the child kind gets
                        // the canonical (null-theme) rendering — themes are
                        // partial by design (§5e step 4).
                        let target = el.fragment.as_deref().unwrap_or(child);
                        if target != child {
                            bail!(
                                "{file}:{}: `{slot}` holds `{child}` maps, but \
                                 data-fragment=\"{target}\" binds a different kind",
                                el.line
                            );
                        }
                    }
                }
            } else if let Some(f) = &el.fragment {
                bail!(
                    "{file}:{}: data-fragment=\"{f}\" without data-slot",
                    el.line
                );
            }
            for a in &el.attrs {
                if let Attr::Slot(attr, part) = a {
                    match crate::parts::part_type(kind, part) {
                        Some(PartType::Text | PartType::Url) => {}
                        Some(_) => bail!(
                            "{file}:{}: data-slot-{attr} must name a text part, \
                             `{part}` is not one",
                            el.line
                        ),
                        None => bail!(
                            "{file}:{}: unknown part `{part}` in data-slot-{attr} — \
                             `{kind}` has: {}",
                            el.line,
                            known_parts(kind)
                        ),
                    }
                }
            }
            self.validate_nodes(&el.children, kind, file)?;
        }
        Ok(())
    }

    fn known_fragments(&self) -> String {
        let v: Vec<&str> = self.map.keys().map(String::as_str).collect();
        if v.is_empty() { "(none)".into() } else { v.join(", ") }
    }

    /// `(slot name, element tag)` for every content hole in a fragment — the
    /// block-arity rule (§5e tree-filled slots) needs to know whether a slot
    /// element takes flow content or only phrasing.
    pub fn slot_tags(&self, kind: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        if let Some(f) = self.map.get(kind) {
            collect_slot_tags(&f.nodes, &mut out);
        }
        out
    }

    // -------------------------------------------------------------- render

    /// Render the map through its kind's fragment — or, when the theme
    /// declines to arrange this kind, through the canonical null rendering
    /// (§5e step 4). Themes are partial: a theme with *no* fragments is the
    /// null theme, and it needs no directory at all. Infallible once `load`
    /// has validated.
    pub fn render(&self, m: &PartMap) -> String {
        match self.map.get(m.kind) {
            Some(frag) => {
                let mut out = String::new();
                self.render_nodes(&frag.nodes, m, &mut out, true);
                out
            }
            None => crate::parts::canonical(m),
        }
    }

    fn render_nodes(&self, nodes: &[Node], m: &PartMap, out: &mut String, mut root: bool) {
        for n in nodes {
            match n {
                Node::Text(t) => out.push_str(t),
                Node::Element(el) => {
                    // The first element of a fragment is its root: stamp
                    // data-kind + the map's true flags for theme CSS.
                    self.render_element(el, m, out, root);
                    root = false;
                }
            }
        }
    }

    fn render_element(&self, el: &Element, m: &PartMap, out: &mut String, root: bool) {
        // Rule 2: an empty part deletes its element.
        let part = match &el.slot {
            Some(slot) => match m.get(slot) {
                None => return,
                Some(Part::Text(s)) if s.is_empty() => return,
                Some(Part::Html(s)) if s.is_empty() => return,
                Some(Part::Stream(v)) if v.is_empty() => return,
                p => p,
            },
            None => None,
        };

        out.push('<');
        out.push_str(&el.tag);
        for a in &el.attrs {
            match a {
                Attr::Literal(name, None) => {
                    let _ = write!(out, " {name}");
                }
                Attr::Literal(name, Some(v)) => {
                    let _ = write!(out, " {name}=\"{v}\"");
                }
                // Rule 3: fill the attribute, or omit it wholesale.
                Attr::Slot(attr, pname) => {
                    if let Some(Part::Text(v)) = m.get(pname) {
                        if !v.is_empty() {
                            let _ = write!(out, " {attr}=\"{}\"", esc(v));
                        }
                    }
                }
            }
        }
        if let Some(slot) = &el.slot {
            let _ = write!(out, " data-slot=\"{slot}\"");
        }
        if root {
            let _ = write!(out, " data-kind=\"{}\"", m.kind);
            for (name, p) in m.iter() {
                if matches!(p, Part::Flag(true)) {
                    let _ = write!(out, " data-{name}");
                }
            }
        }
        if el.void {
            out.push('>');
            return;
        }
        out.push('>');

        // Rule 1: a slotted element's content is the part, not its children.
        match part {
            Some(Part::Text(s)) => out.push_str(&esc(s)),
            Some(Part::Html(s)) => out.push_str(s),
            Some(Part::Stream(v)) => {
                for item in v {
                    match self.map.get(item.kind) {
                        Some(child) => self.render_nodes(&child.nodes, item, out, true),
                        None => out.push_str(&crate::parts::canonical(item)),
                    }
                }
            }
            Some(Part::Map(sub)) => match self.map.get(sub.kind) {
                Some(child) => self.render_nodes(&child.nodes, sub, out, true),
                None => out.push_str(&crate::parts::canonical(sub)),
            },
            Some(Part::Flag(_)) => unreachable!("flags cannot fill content (validated)"),
            None => self.render_nodes(&el.children, m, out, false),
        }

        let _ = write!(out, "</{}>", el.tag);
    }
}

fn collect_slot_tags(nodes: &[Node], out: &mut Vec<(String, String)>) {
    for n in nodes {
        if let Node::Element(el) = n {
            if let Some(s) = &el.slot {
                out.push((s.clone(), el.tag.clone()));
            }
            collect_slot_tags(&el.children, out);
        }
    }
}

/// Elements whose content model is phrasing-only: a markdown fill landing in
/// one of these must be exactly one block, which unwraps to its inline
/// content (§5e's block-arity rule).
pub fn is_phrasing_only(tag: &str) -> bool {
    matches!(
        tag,
        "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "span" | "a" | "em"
            | "strong" | "small" | "time" | "abbr" | "b" | "i" | "code" | "label"
    )
}

fn known_parts(kind: &str) -> String {
    crate::parts::schema(kind)
        .map(|s| s.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", "))
        .unwrap_or_default()
}

fn known_kinds() -> &'static str {
    "document, listing, summary, link_list, link, crumb, tag, relation, \
     neighbor, pagination, page_link, raw"
}

// ------------------------------------------------------------------ parser

const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta",
    "param", "source", "track", "wbr",
];

struct Parser<'a> {
    s: &'a str,
    pos: usize,
    file: &'a str,
}

impl<'a> Parser<'a> {
    fn new(s: &'a str, file: &'a str) -> Parser<'a> {
        Parser { s, pos: 0, file }
    }

    fn line(&self) -> usize {
        self.s[..self.pos].bytes().filter(|&b| b == b'\n').count() + 1
    }

    fn err(&self, msg: &str) -> anyhow::Error {
        anyhow::anyhow!("{}:{}: {msg}", self.file, self.line())
    }

    fn rest(&self) -> &'a str {
        &self.s[self.pos..]
    }

    fn eat(&mut self, prefix: &str) -> bool {
        if self.rest().starts_with(prefix) {
            self.pos += prefix.len();
            true
        } else {
            false
        }
    }

    fn parse_all(&mut self) -> Result<Vec<Node>> {
        let nodes = self.parse_nodes()?;
        if !self.rest().is_empty() {
            return Err(self.err("unexpected close tag with no open element"));
        }
        Ok(nodes)
    }

    /// Parse siblings until EOF or a close tag (left for the caller).
    fn parse_nodes(&mut self) -> Result<Vec<Node>> {
        let mut nodes = Vec::new();
        loop {
            if self.rest().is_empty() || self.rest().starts_with("</") {
                return Ok(nodes);
            }
            if self.rest().starts_with("<!--") {
                let end = self
                    .rest()
                    .find("-->")
                    .ok_or_else(|| self.err("unterminated comment"))?;
                nodes.push(Node::Text(self.rest()[..end + 3].to_string()));
                self.pos += end + 3;
            } else if self.rest().starts_with("<!") {
                // doctype, verbatim
                let end = self
                    .rest()
                    .find('>')
                    .ok_or_else(|| self.err("unterminated <! declaration"))?;
                nodes.push(Node::Text(self.rest()[..end + 1].to_string()));
                self.pos += end + 1;
            } else if self.rest().starts_with('<') {
                nodes.push(Node::Element(self.parse_element()?));
            } else {
                let end = self.rest().find('<').unwrap_or(self.rest().len());
                nodes.push(Node::Text(self.rest()[..end].to_string()));
                self.pos += end;
            }
        }
    }

    fn parse_element(&mut self) -> Result<Element> {
        let line = self.line();
        assert!(self.eat("<"));
        let tag = self.ident()?;
        if tag.is_empty() {
            return Err(self.err("expected a tag name after `<`"));
        }
        let mut el = Element {
            tag: tag.clone(),
            attrs: Vec::new(),
            slot: None,
            fragment: None,
            children: Vec::new(),
            void: VOID.contains(&tag.as_str()),
            line,
        };
        // attributes
        loop {
            self.skip_ws();
            if self.eat("/>") {
                if !el.void && !el.children.is_empty() {
                    unreachable!()
                }
                el.void = true; // self-closed: no children, no close tag
                return Ok(el);
            }
            if self.eat(">") {
                break;
            }
            let name = self.ident()?;
            if name.is_empty() {
                return Err(self.err("expected an attribute name"));
            }
            let value = if self.eat("=") {
                if !self.eat("\"") {
                    return Err(self.err(&format!(
                        "attribute `{name}` must be double-quoted"
                    )));
                }
                let end = self
                    .rest()
                    .find('"')
                    .ok_or_else(|| self.err("unterminated attribute value"))?;
                let v = self.rest()[..end].to_string();
                self.pos += end + 1;
                Some(v)
            } else {
                None
            };
            match name.as_str() {
                "data-slot" => {
                    el.slot = Some(value.ok_or_else(|| self.err("data-slot needs a value"))?)
                }
                "data-fragment" => {
                    el.fragment =
                        Some(value.ok_or_else(|| self.err("data-fragment needs a value"))?)
                }
                _ if name.starts_with("data-slot-") => {
                    let attr = name["data-slot-".len()..].to_string();
                    let part =
                        value.ok_or_else(|| self.err(&format!("{name} needs a value")))?;
                    el.attrs.push(Attr::Slot(attr, part));
                }
                _ => el.attrs.push(Attr::Literal(name, value)),
            }
        }
        if el.void {
            return Ok(el);
        }
        // Raw-text elements: the content is opaque up to the literal close tag.
        if el.tag == "script" || el.tag == "style" {
            let close = format!("</{}>", el.tag);
            let end = self
                .rest()
                .find(&close)
                .ok_or_else(|| self.err(&format!("unterminated <{}>", el.tag)))?;
            el.children.push(Node::Text(self.rest()[..end].to_string()));
            self.pos += end + close.len();
            return Ok(el);
        }
        el.children = self.parse_nodes()?;
        if !self.eat("</") {
            return Err(self.err(&format!("<{}> is never closed", el.tag)));
        }
        let close = self.ident()?;
        if close != el.tag {
            return Err(self.err(&format!("</{close}> closes <{}>", el.tag)));
        }
        self.skip_ws();
        if !self.eat(">") {
            return Err(self.err("malformed close tag"));
        }
        Ok(el)
    }

    fn ident(&mut self) -> Result<String> {
        let end = self
            .rest()
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':'))
            .unwrap_or(self.rest().len());
        let id = self.rest()[..end].to_string();
        self.pos += end;
        Ok(id)
    }

    fn skip_ws(&mut self) {
        let end = self
            .rest()
            .find(|c: char| !c.is_whitespace())
            .unwrap_or(self.rest().len());
        self.pos += end;
    }
}

// ------------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parts::{Part, PartMap};

    fn frags(sources: &[(&str, &str)]) -> Result<Fragments> {
        Fragments::load(
            sources
                .iter()
                .map(|(k, s)| (k.to_string(), s.to_string(), format!("{k}.html")))
                .collect(),
        )
    }

    fn crumb(label: &str, url: Option<&str>) -> PartMap {
        let mut c = PartMap::new("crumb");
        c.set("label", Part::Text(label.into()));
        if let Some(u) = url {
            c.set("url", Part::Text(u.into()));
        }
        c
    }

    #[test]
    fn scalar_fill_escapes_and_html_is_trusted() {
        let f = frags(&[(
            "summary",
            r#"<article><h2 data-slot="title"></h2><section data-slot="content"></section></article>"#,
        )])
        .unwrap();
        let mut m = PartMap::new("summary");
        m.set("title", Part::Text("A <b> & B".into()));
        m.set("content", Part::Html("<p>hi</p>".into()));
        let out = f.render(&m);
        assert_eq!(
            out,
            r#"<article data-kind="summary"><h2 data-slot="title">A &lt;b&gt; &amp; B</h2><section data-slot="content"><p>hi</p></section></article>"#
        );
    }

    #[test]
    fn empty_part_deletes_element() {
        let f = frags(&[(
            "summary",
            r#"<article><time data-slot="date"></time><section data-slot="content"></section></article>"#,
        )])
        .unwrap();
        let mut m = PartMap::new("summary");
        m.set("content", Part::Html("x".into()));
        let out = f.render(&m);
        assert!(!out.contains("<time"), "{out}");
        // And an empty stream deletes its container too.
        let f = frags(&[
            ("summary", r#"<article><div data-slot="tags"></div></article>"#),
            ("tag", r#"<a data-slot="name"></a>"#),
        ])
        .unwrap();
        let m = PartMap::new("summary");
        assert_eq!(f.render(&m), r#"<article data-kind="summary"></article>"#);
    }

    #[test]
    fn stream_maps_child_fragment_per_item() {
        let f = frags(&[
            ("document", r#"<nav data-slot="crumbs"></nav>"#),
            ("crumb", r#"<span><a data-slot-href="url" data-slot="label"></a></span>"#),
        ])
        .unwrap();
        let mut m = PartMap::new("document");
        m.set(
            "crumbs",
            Part::Stream(vec![crumb("Home", Some("/")), crumb("16", None)]),
        );
        let out = f.render(&m);
        // Linked crumb gets href; the inert tail is a placeholder link —
        // `<a>` with no href, selectable as a:not([href]).
        assert_eq!(
            out,
            r#"<nav data-slot="crumbs" data-kind="document"><span data-kind="crumb"><a href="/" data-slot="label">Home</a></span><span data-kind="crumb"><a data-slot="label">16</a></span></nav>"#
        );
    }

    #[test]
    fn map_part_renders_named_fragment() {
        let f = frags(&[
            ("listing", r#"<div><nav data-slot="pagination"></nav></div>"#),
            (
                "pagination",
                r#"<div><a data-slot-href="prev">Prev</a><ol data-slot="pages"></ol></div>"#,
            ),
            ("page_link", r#"<li><a data-slot-href="url" data-slot="n"></a></li>"#),
        ])
        .unwrap();
        let mut m = PartMap::new("listing");
        m.set("pagination", Part::Map(crate::parts::pagination(2, 3).unwrap()));
        let out = f.render(&m);
        assert!(out.contains(r#"<a href="/blog/">Prev</a>"#), "{out}");
        assert!(out.contains(r#"<li data-kind="page_link"><a data-slot="n">2</a></li>"#), "{out}");
        assert!(out.contains(r#"<a href="/blog/page/3" data-slot="n">3</a>"#), "{out}");
    }

    #[test]
    fn facts_stamp_data_attributes_on_root() {
        let f = frags(&[(
            "document",
            r#"<article><section data-slot="content"></section></article>"#,
        )])
        .unwrap();
        let mut m = PartMap::new("document");
        m.set("tree", Part::Flag(true));
        m.set("content", Part::Html("x".into()));
        let out = f.render(&m);
        assert!(out.starts_with(r#"<article data-kind="document" data-tree>"#), "{out}");
    }

    #[test]
    fn unknown_slot_is_a_load_error_naming_the_knowns() {
        let e = frags(&[("summary", r#"<h2 data-slot="titel"></h2>"#)]).unwrap_err();
        let msg = format!("{e}");
        assert!(msg.contains("summary.html:1"), "{msg}");
        assert!(msg.contains("unknown slot `titel`"), "{msg}");
        assert!(msg.contains("title, url, date"), "{msg}");
    }

    /// Themes are partial (§5e step 4): a kind the theme declines to arrange
    /// renders canonically — the null theme is the fallback, not an error.
    #[test]
    fn missing_child_fragment_falls_back_to_canonical() {
        let f = frags(&[("document", r#"<nav data-slot="crumbs"></nav>"#)]).unwrap();
        let mut m = PartMap::new("document");
        m.set(
            "crumbs",
            Part::Stream(vec![crumb("Home", Some("/")), crumb("16", None)]),
        );
        let out = f.render(&m);
        assert!(out.contains(r#"<section data-kind="crumb">"#), "{out}");
        assert!(out.contains(r#"<a data-slot="url" href="/">/</a>"#), "{out}");
        assert!(out.contains(r#"<span data-slot="label">16</span>"#), "{out}");
    }

    /// And a map whose own kind has no fragment renders canonically wholesale
    /// — a theme with no fragments at all IS the null theme.
    #[test]
    fn missing_root_fragment_is_the_null_theme() {
        let f = frags(&[]).unwrap();
        let mut m = PartMap::new("summary");
        m.set("title", Part::Text("T".into()));
        m.set("content", Part::Html("<p>x</p>".into()));
        let out = f.render(&m);
        assert!(out.starts_with(r#"<section data-kind="summary">"#), "{out}");
        assert!(out.contains(r#"<div data-slot="content"><p>x</p></div>"#), "{out}");
    }

    #[test]
    fn fact_slot_and_void_content_slot_are_load_errors() {
        let e = frags(&[("document", r#"<b data-slot="tree"></b>"#)]).unwrap_err();
        assert!(format!("{e}").contains("is a fact"), "{e}");
        let e = frags(&[("summary", r#"<img data-slot="title">"#)]).unwrap_err();
        assert!(format!("{e}").contains("void element <img>"), "{e}");
    }

    #[test]
    fn attr_slot_must_name_a_text_part() {
        let e = frags(&[("summary", r#"<a data-slot-href="tags">x</a>"#)]).unwrap_err();
        assert!(format!("{e}").contains("must name a text part"), "{e}");
    }

    #[test]
    fn malformed_html_is_a_load_error() {
        for (src, want) in [
            (r#"<div><p>x</div>"#, "closes <p>"),
            (r#"<div>x"#, "never closed"),
            (r#"<div class=x>y</div>"#, "double-quoted"),
        ] {
            let e = frags(&[("raw", src)]).unwrap_err();
            assert!(format!("{e}").contains(want), "{src} → {e}");
        }
    }

    #[test]
    fn comments_doctype_script_pass_through() {
        let f = frags(&[(
            "raw",
            "<!doctype html>\n<!-- c -->\n<div><script>if (1 < 2) x();</script><section data-slot=\"content\"></section></div>",
        )])
        .unwrap();
        let mut m = PartMap::new("raw");
        m.set("content", Part::Html("y".into()));
        let out = f.render(&m);
        assert!(out.contains("<!doctype html>"), "{out}");
        assert!(out.contains("<!-- c -->"), "{out}");
        assert!(out.contains("if (1 < 2) x();"), "{out}");
    }

    #[test]
    fn unknown_kind_filename_is_a_load_error() {
        let e = frags(&[("sidebar", "<div></div>")]).unwrap_err();
        assert!(format!("{e}").contains("no layout kind `sidebar`"), "{e}");
    }
}

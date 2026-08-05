//! The fragment binder (THEME.md §5): parse theme HTML once, validate slots
//! against schemas, fill from part maps at render.
//!
//! Hole algebra: `data-slot` replaces content (empty → delete element);
//! `data-slot-attr` sets/omits attributes; a source-less media element and an
//! empty wrapper both collapse (rules 2b/2c below), unless the wrapper carries
//! `data-no-collapse`; flags stamp `data-*` on the root.
//! Load-time checks only — render is infallible.

use anyhow::{bail, Result};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use super::parts::{Part, PartMap, PartType};
use crate::render::esc;

#[derive(Debug)]
enum Node {
    /// Verbatim output: text, comments, doctype — one variant because they
    /// all render the same way, byte for byte. `line` is where the run
    /// starts; only `split_root`'s top-level rules read it, and they need it
    /// for the same reason every other error here names a line.
    Text {
        text: String,
        line: usize,
    },
    Element(Element),
}

/// The three flavours of `Node::Text`, which the top level of a theme root
/// has to tell apart: a comment is nothing to see, a doctype is the wrapper
/// mistake, and authored words are a drop waiting to happen. Spelled as
/// prefixes because that is exactly what the parser branched on to make them.
fn is_comment(text: &str) -> bool {
    text.starts_with("<!--")
}

fn is_doctype(text: &str) -> bool {
    text.starts_with("<!") && !is_comment(text)
}

/// Enough of a text run to recognize it in the file, on one line: whitespace
/// collapsed, forty characters, an ellipsis if there was more. An error about
/// dropped words has to quote some of them or its reader is hunting.
fn snippet(text: &str) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let short: String = flat.chars().take(40).collect();
    if short.chars().count() < flat.chars().count() {
        format!("{short}…")
    } else {
        short
    }
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
    /// `data-no-collapse` — keep this element even when it renders empty
    /// (rule 2c's opt-out). A frame a theme wants present regardless.
    no_collapse: bool,
    children: Vec<Node>,
    void: bool,
    line: usize,
    /// Source span of this element's CHILDREN — the bytes between its open
    /// and close tags. Only `split_root` reads it: a document-shaped theme
    /// root hands its two halves on as *sources*, and slicing them out is
    /// how the head fence and the chrome fragment can be checked by the
    /// same parser that already read the file.
    inner: std::ops::Range<usize>,
}

#[derive(Debug)]
enum Attr {
    /// `name="value"` or bare `name`, emitted verbatim.
    Literal(String, Option<String>),
    /// `data-slot-<attr>="part"`: fill `attr` from a Text part, or omit.
    Slot(String, String),
}

/// One parsed fragment file. The file stem is its NAME; the part of the
/// stem before `--` is the KIND it binds to (q24 variants: `summary.html`
/// and `row--card.html` both bind `row`; a view's `variant`
/// selects between them).
#[derive(Debug)]
pub struct Fragment {
    pub kind: String,
    nodes: Vec<Node>,
}

/// A theme's fragment set, keyed by fragment NAME. Loading validates every
/// fragment against the part schemas; after that, `render` cannot fail.
#[derive(Debug, Default)]
pub struct Fragments {
    map: BTreeMap<String, Fragment>,
    /// Display path per fragment name (for validate errors).
    files: BTreeMap<String, String>,
    /// Fragment names in load order (base first, then theme).
    order: Vec<String>,
}

/// One hole seen while deriving part schemas from fragments.
#[derive(Debug, Clone)]
pub struct SlotUse {
    pub part: String,
    /// `data-fragment` target, when present.
    pub fragment: Option<String>,
    /// Used as `data-slot-href` / `data-slot-src` (url-shaped attr).
    pub as_url_attr: bool,
    /// Used as a content `data-slot`.
    pub as_content: bool,
}

impl Fragments {
    /// Whether a fragment name is present (base or theme).
    pub fn has(&self, name: &str) -> bool {
        self.map.contains_key(name)
    }

    /// Whether `row--{face}` is present.
    pub fn has_face(&self, face: &str) -> bool {
        self.has(&format!("row--{face}"))
    }

    /// Member face names shipped as `row--*` fragments (sorted).
    pub fn faces(&self) -> Vec<&str> {
        let mut faces: Vec<&str> = self
            .map
            .keys()
            .filter_map(|n| n.strip_prefix("row--"))
            .collect();
        faces.sort_unstable();
        faces
    }

    /// Kind names present (fragment stems before `--`).
    pub fn kinds(&self) -> Vec<&str> {
        let mut kinds: Vec<&str> = self.map.keys().map(|n| kind_of_name(n)).collect();
        kinds.sort_unstable();
        kinds.dedup();
        kinds
    }

    /// Parse without schema validation — used when deriving schemas from
    /// the fragments themselves (THEME.md §5). Extracts optional inline
    /// fragment defaults from stream/map holes before returning.
    pub fn parse(sources: Vec<(String, String, String)>) -> Result<Fragments> {
        let mut f = Fragments::default();
        for (name, text, file) in &sources {
            let kind = kind_of_name(name).to_string();
            let nodes = Parser::new(text, file).parse_all()?;
            f.map.insert(name.clone(), Fragment { kind, nodes });
            f.files.insert(name.clone(), file.clone());
            f.order.push(name.clone());
        }
        f.extract_inlines();
        Ok(f)
    }

    /// Replace or insert fragments from `sources`, then re-extract inlines.
    /// Used when a theme overlays the base: inline defaults harvested from a
    /// base parent survive even if the theme replaces that parent file, unless
    /// the theme also ships a fragment of the same name (file wins).
    pub fn overlay(&mut self, sources: Vec<(String, String, String)>) -> Result<()> {
        for (name, text, file) in &sources {
            let kind = kind_of_name(name).to_string();
            let nodes = Parser::new(text, file).parse_all()?;
            if !self.map.contains_key(name) {
                self.order.push(name.clone());
            }
            self.map.insert(name.clone(), Fragment { kind, nodes });
            self.files.insert(name.clone(), file.clone());
        }
        self.extract_inlines();
        Ok(())
    }

    /// Pull default bodies out of stream/map holes into the fragment map.
    /// A hole with `data-fragment="NAME"` and element children defines `NAME`
    /// when no file already provided it; a file always wins (children dropped).
    /// A later inline replaces an earlier one (theme over base).
    fn extract_inlines(&mut self) {
        let mut i = 0;
        while i < self.order.len() {
            let name = self.order[i].clone();
            self.extract_inlines_in(&name);
            i += 1;
        }
    }

    fn extract_inlines_in(&mut self, name: &str) {
        let Some(frag) = self.map.get_mut(name) else {
            return;
        };
        let mut nodes = std::mem::take(&mut frag.nodes);
        let parent_file = self
            .files
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string());
        extract_inline_nodes(&mut nodes, self, &parent_file);
        if let Some(frag) = self.map.get_mut(name) {
            frag.nodes = nodes;
        }
    }

    /// Check every fragment against `schemas` (unknown kind / slot / arity).
    pub fn validate_against(&self, schemas: &crate::parts::Schemas) -> Result<()> {
        for name in &self.order {
            let frag = &self.map[name];
            let file = self.files.get(name).map(String::as_str).unwrap_or(name);
            if schemas.get(&frag.kind).is_none() {
                bail!(
                    "{file}: fragment names no layout kind `{}` — kinds are: {}",
                    frag.kind,
                    known_kinds(schemas)
                );
            }
            self.validate(schemas, frag, file)?;
        }
        Ok(())
    }

    /// Slot uses per kind, in first-seen document order across fragments.
    pub fn slot_uses(&self) -> BTreeMap<String, Vec<SlotUse>> {
        let mut out: BTreeMap<String, Vec<SlotUse>> = BTreeMap::new();
        for name in &self.order {
            let frag = &self.map[name];
            let entry = out.entry(frag.kind.clone()).or_default();
            collect_slot_uses(&frag.nodes, entry);
        }
        out
    }

    /// Load from `(name, source, display-name)` triples.
    pub fn load(
        sources: Vec<(String, String, String)>,
        schemas: &crate::parts::Schemas,
    ) -> Result<Fragments> {
        let f = Fragments::parse(sources)?;
        f.validate_against(schemas)?;
        Ok(f)
    }

    fn validate(&self, schemas: &crate::parts::Schemas, frag: &Fragment, file: &str) -> Result<()> {
        self.validate_nodes(schemas, &frag.nodes, &frag.kind, file)
    }
}

/// Walk a node tree: stream/map holes with element children become named
/// fragments when `NAME` is not already file-backed. A later inline replaces
/// an earlier one (theme overlay over base); a real file always wins.
fn extract_inline_nodes(nodes: &mut Vec<Node>, frags: &mut Fragments, parent_file: &str) {
    for n in nodes.iter_mut() {
        let Node::Element(el) = n else { continue };
        let Some(fname) = el.fragment.clone() else {
            extract_inline_nodes(&mut el.children, frags, parent_file);
            continue;
        };
        if el.slot.is_none() {
            extract_inline_nodes(&mut el.children, frags, parent_file);
            continue;
        }
        if !has_element_child(&el.children) {
            extract_inline_nodes(&mut el.children, frags, parent_file);
            continue;
        }
        if let Some(existing) = frags.files.get(&fname) {
            if !is_inline_origin(existing) {
                el.children.clear();
                continue;
            }
            frags.map.remove(&fname);
            frags.files.remove(&fname);
            frags.order.retain(|n| n != &fname);
        }
        let children = std::mem::take(&mut el.children);
        let kind = kind_of_name(&fname).to_string();
        frags.map.insert(
            fname.clone(),
            Fragment {
                kind,
                nodes: children,
            },
        );
        frags
            .files
            .insert(fname.clone(), format!("{parent_file} (inline `{fname}`)"));
        frags.order.push(fname.clone());
        // Nested inlines inside the new body (order walk picks it up too;
        // extract now so a same-pass sibling can see the name).
        frags.extract_inlines_in(&fname);
    }
}

fn is_inline_origin(file: &str) -> bool {
    file.contains(" (inline `")
}

fn has_element_child(nodes: &[Node]) -> bool {
    nodes.iter().any(|n| matches!(n, Node::Element(_)))
}

/// `row--card` → the `row` schema.
fn kind_of_name(name: &str) -> &str {
    name.split_once("--").map(|(k, _)| k).unwrap_or(name)
}

/// Every `*.html` in a theme directory, as `(name, source, display)` triples.
/// The file stem names the fragment; its prefix names the kind it binds to.
/// Separate from `Fragments::load` because a theme's sources are only the
/// TOP layer — the engine's base theme supplies the rest (`theme.rs`).
pub fn dir_sources(dir: &Path) -> Result<Vec<(String, String, String)>> {
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
    Ok(sources)
}

/// A theme root's two halves. `style` is the CSS the theme's
/// `<head>` declared — the *contents* of its `<style>` elements, not the
/// head markup, because the head is fenced to `<style>` and the one thing
/// downstream does with it is compile it into the site's CSS. `body` is
/// the chrome the binder loads as the `root` kind. Either may be absent —
/// `None` on both is not reachable, since a file with neither wrapper IS the
/// body.
pub struct Root {
    pub style: Option<String>,
    pub body: Option<String>,
}

/// Split `root.html` into head styles and body chrome.
///
/// Accepts a bare body fragment, or top-level `<head>`/`<body>`. Refuses
/// `<html>`/doctype — the engine owns the skeleton; wrapping would
/// hide head/body and ship theme metas inside `<body>`.
pub fn split_root(src: &str, file: &str) -> Result<Root> {
    let nodes = Parser::new(src, file).parse_all()?;
    // Before anything else, because this is the check the two shapes below
    // cannot make for themselves: an `<html>` wrapper would sail past the
    // `wrapped` test as a fragment, and a doctype would sail past it as
    // verbatim text. One message for both — they are one mistake, and the
    // fix for it is one edit.
    for n in &nodes {
        let (what, line) = match n {
            Node::Element(el) if el.tag == "html" => ("<html>", el.line),
            Node::Text { text, line } if is_doctype(text) => ("a doctype", *line),
            _ => continue,
        };
        bail!(
            "{file}:{line}: {what} at the top of a theme root — the engine \
             writes the document skeleton itself: the doctype, and <html> \
             with its lang, subtheme, profile and axis stamps. \
             Unwrap to a bare <head>/<body> pair, or to the body chrome \
             alone. Inside an <html> the head and body are invisible to \
             this split, so the whole file would become body chrome and the \
             theme's <title> and <meta> would ship inside <body> on every \
             page."
        );
    }
    let wrapped = nodes
        .iter()
        .any(|n| matches!(n, Node::Element(el) if el.tag == "head" || el.tag == "body"));
    if !wrapped {
        return Ok(Root {
            style: None,
            body: Some(src.to_string()),
        });
    }
    let mut root = Root {
        style: None,
        body: None,
    };
    for n in &nodes {
        let el = match n {
            Node::Element(el) => el,
            // Whitespace and comments between the halves mean nothing and go
            // nowhere; authored words mean something and have nowhere to go,
            // because a document's top level is not a place to put text. So
            // they are named rather than dropped in silence.
            Node::Text { text, line } => {
                if !is_comment(text) && !text.trim().is_empty() {
                    // A text run starts at the newline after `</head>`; the
                    // words are what the reader is looking for, so the line
                    // reported is theirs and not the run's.
                    let lead = text.len() - text.trim_start().len();
                    let at = line + text[..lead].bytes().filter(|&b| b == b'\n').count();
                    bail!(
                        "{file}:{at}: text beside a theme root's <head>/<body> \
                         (`{}`) — a document-shaped root holds those two and \
                         nothing else, and the engine writes <html> itself. \
                         Move it inside <body>.",
                        snippet(text)
                    );
                }
                continue;
            }
        };
        let (slot, take) = match el.tag.as_str() {
            "head" => {
                check_head_fence(el, file)?;
                // The head half leaves as CSS, not as markup: the fence has
                // just proved there is nothing else in it, so carrying the
                // `<style>` tags on would only mean stripping them at the
                // other end (`shells::css::css_pass`).
                (&mut root.style, head_styles(el, src))
            }
            "body" => (&mut root.body, src[el.inner.clone()].to_string()),
            // A `<style>` here is the one sibling with an obvious home that
            // is NOT the body: the head fence exists to take it (the CSS assembly compiles
            // it into the theme's sheet), so pointing at `<body>` would be
            // advice that lands the theme's CSS in its chrome.
            _ => bail!(
                "{file}:{}: <{}> beside a theme root's <head>/<body> — a \
                 document-shaped root holds those two and nothing else, and \
                 the engine writes <html> itself. Move it inside \
                 {}.",
                el.line,
                el.tag,
                if el.tag == "style" {
                    "<head>"
                } else {
                    "<body>"
                }
            ),
        };
        if slot.is_some() {
            bail!("{file}:{}: a second <{}>", el.line, el.tag);
        }
        *slot = Some(take);
    }
    Ok(root)
}

/// The CSS a theme root's `<head>` declares: every `<style>` element's
/// contents, in source order, newline-joined. Everything else in the head —
/// comments, stray text, and (unreachably, because `check_head_fence` ran
/// first) any other element — is dropped: the head is a declaration of
/// styles, and its markup is the engine's.
///
/// The `style` test is deliberately *stated* rather than left to the fence.
/// This function's contract is "the CSS", and a function whose correctness
/// depends on a check made by its caller two frames up is one refactor from
/// being wrong.
fn head_styles(head: &Element, src: &str) -> String {
    let mut out = String::new();
    for n in &head.children {
        let Node::Element(el) = n else { continue };
        if el.tag != "style" {
            continue;
        }
        let text = src[el.inner.clone()].trim();
        if text.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(text);
    }
    out
}

/// The head fence: a theme root's `<head>` may hold `<style>` and
/// nothing else. Every other element is a load error naming the file and the
/// element, because the engine computes the head — a theme writing its own
/// `<title>` or `canonical` link would be shadowed by the engine's on every
/// page, silently, and a theme writing a second stylesheet `<link>` would
/// break the one-artifact rule the CSS assembly is built on.
///
/// The allowlist principle is "presentational head elements", and it starts
/// at one; `<meta name="theme-color">` is the known first candidate to widen
/// it, when a real theme hits the wall.
fn check_head_fence(head: &Element, file: &str) -> Result<()> {
    for n in &head.children {
        let Node::Element(el) = n else { continue };
        if el.tag != "style" {
            bail!(
                "{file}:{}: <{}> in a theme root's <head> — a theme head may hold \
                 <style> and nothing else. The engine computes the head: \
                 the title, the charset, the canonical link, the [html.head.*] \
                 tables, hreflang, and the one stylesheet link.",
                el.line,
                el.tag
            );
        }
    }
    Ok(())
}

impl Fragments {
    fn validate_nodes(
        &self,
        schemas: &crate::parts::Schemas,
        nodes: &[Node],
        kind: &str,
        file: &str,
    ) -> Result<()> {
        for n in nodes {
            let Node::Element(el) = n else { continue };
            if let Some(slot) = &el.slot {
                let Some(ty) = schemas
                    .get(kind)
                    .and_then(|s| s.iter().find(|(n, _)| n == slot).map(|(_, t)| *t))
                else {
                    bail!(
                        "{file}:{}: unknown slot `{slot}` on <{}> — `{kind}` has: {}",
                        el.line,
                        el.tag,
                        known_parts(schemas, kind)
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
                        // Default child rendering falls back to canonical
                        // when the theme has no fragment (partial themes,
                        // §5e step 4) — but an EXPLICIT data-fragment must
                        // resolve, and to the right kind (q24 variants).
                        if let Some(target) = el.fragment.as_deref() {
                            match self.map.get(target) {
                                None => bail!(
                                    "{file}:{}: data-fragment=\"{target}\" names no \
                                     fragment — have: {}",
                                    el.line,
                                    self.known_fragments()
                                ),
                                Some(frag) if frag.kind != child => bail!(
                                    "{file}:{}: `{slot}` holds `{child}` maps, but \
                                     data-fragment=\"{target}\" binds kind `{}`",
                                    el.line,
                                    frag.kind
                                ),
                                Some(_) => {}
                            }
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
                    match schemas
                        .get(kind)
                        .and_then(|s| s.iter().find(|(n, _)| n == part).map(|(_, t)| *t))
                    {
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
                            known_parts(schemas, kind)
                        ),
                    }
                }
            }
            self.validate_nodes(schemas, &el.children, kind, file)?;
        }
        Ok(())
    }

    fn known_fragments(&self) -> String {
        let v: Vec<&str> = self.map.keys().map(String::as_str).collect();
        crate::util::join_or_none(&v)
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

    /// Render the map through its kind's fragment — or, when the theme
    /// declines to arrange this kind, through the canonical null rendering
    /// (§5e step 4). Themes are partial: a theme with *no* fragments is the
    /// null theme, and it needs no directory at all. Infallible once `load`
    /// has validated.
    pub fn render(&self, m: &PartMap) -> String {
        self.render_with(m, None)
    }

    /// Render a map as BODY content (§5g): like `render`, but the fragment
    /// root is not stamped — the engine's root HTML shell carries
    /// `data-kind`/`data-subtheme` on `<html>` itself.
    pub fn render_body(&self, m: &PartMap) -> String {
        self.render_frag(m, None, false)
    }

    /// Render through a variant's fragment when the theme has one (q24):
    /// `{kind}--{variant}` → the kind's base fragment → canonical. The
    /// view declares the variant; the theme opts in by shipping the file.
    pub fn render_with(&self, m: &PartMap, variant: Option<&str>) -> String {
        self.render_frag(m, variant, true)
    }

    fn render_frag(&self, m: &PartMap, variant: Option<&str>, stamp_root: bool) -> String {
        let frag = variant
            .and_then(|v| self.map.get(&format!("{}--{v}", m.kind)))
            .or_else(|| self.map.get(m.kind));
        match frag {
            Some(frag) => {
                let mut out = String::new();
                self.render_nodes(&frag.nodes, m, &mut out, stamp_root);
                out
            }
            None => super::parts::canonical(m),
        }
    }

    fn render_nodes(&self, nodes: &[Node], m: &PartMap, out: &mut String, mut root: bool) {
        for n in nodes {
            match n {
                Node::Text { text, .. } => out.push_str(text),
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

        // Rule 2b: a source-less media element deletes itself. An <img> (or
        // <iframe>, <video>…) whose `src` comes from an absent or empty part
        // has nothing to show — the attribute-hole equivalent of rule 2's
        // empty content. A missing `href` on an <a> is deliberately different
        // (rule 3 leaves the inert link, because its text still means
        // something); `src` is the one attribute whose absence empties the
        // element. So an imageless `hero` collapses instead of shipping a
        // broken <img>.
        for a in &el.attrs {
            if let Attr::Slot(attr, pname) = a {
                if attr == "src" && !matches!(m.get(pname), Some(Part::Text(s)) if !s.is_empty()) {
                    return;
                }
            }
        }

        let start = out.len();
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
        let body_start = out.len();

        // Rule 1: a slotted element's content is the part, not its children.
        match part {
            Some(Part::Text(s)) => out.push_str(&esc(s)),
            Some(Part::Html(s)) => out.push_str(s),
            Some(Part::Stream(v)) => {
                for item in v {
                    // The map's own face wins, then the hole's data-fragment,
                    // then the child kind's base fragment, else canonical.
                    let name = item.face().or(el.fragment.as_deref()).unwrap_or(item.kind);
                    match self.map.get(name) {
                        Some(child) => self.render_nodes(&child.nodes, item, out, true),
                        None => out.push_str(&super::parts::canonical(item)),
                    }
                }
            }
            Some(Part::Map(sub)) => {
                let name = sub.face().or(el.fragment.as_deref()).unwrap_or(sub.kind);
                match self.map.get(name) {
                    Some(child) => self.render_nodes(&child.nodes, sub, out, true),
                    None => out.push_str(&super::parts::canonical(sub)),
                }
            }
            Some(Part::Flag(_)) => unreachable!("flags cannot fill content (validated)"),
            None => self.render_nodes(&el.children, m, out, false),
        }

        // Rule 2c: an empty wrapper collapses. A non-slot element that had
        // element children (so it is a wrapper, not an authored-empty `<div>`)
        // but rendered nothing had only holes, and they are all gone — an empty
        // box still paints a theme's border, margin or frame, so rewind past
        // it. This is what makes an imageless hero, a copyright-less footer, or
        // an empty section leave no trace; it propagates bottom-up through
        // nesting. A theme keeps a frame it wants present regardless with
        // `data-no-collapse`, and the root element never collapses (it always
        // carries the document).
        if el.slot.is_none()
            && !root
            && !el.no_collapse
            && out[body_start..].trim().is_empty()
            && el.children.iter().any(|n| matches!(n, Node::Element(_)))
        {
            out.truncate(start);
            return;
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

fn collect_slot_uses(nodes: &[Node], out: &mut Vec<SlotUse>) {
    for n in nodes {
        let Node::Element(el) = n else { continue };
        for a in &el.attrs {
            if let Attr::Slot(attr, part) = a {
                let is_url = attr == "href" || attr == "src";
                merge_slot_use(out, part, None, is_url, false);
            }
        }
        if let Some(slot) = &el.slot {
            merge_slot_use(out, slot, el.fragment.clone(), false, true);
        }
        collect_slot_uses(&el.children, out);
    }
}

fn merge_slot_use(
    out: &mut Vec<SlotUse>,
    part: &str,
    fragment: Option<String>,
    as_url_attr: bool,
    as_content: bool,
) {
    if let Some(u) = out.iter_mut().find(|u| u.part == part) {
        if u.fragment.is_none() {
            u.fragment = fragment;
        }
        u.as_url_attr |= as_url_attr;
        u.as_content |= as_content;
    } else {
        out.push(SlotUse {
            part: part.to_string(),
            fragment,
            as_url_attr,
            as_content,
        });
    }
}

/// Elements whose content model is phrasing-only: a markdown fill landing in
/// one of these must be exactly one block, which unwraps to its inline
/// content (§5e's block-arity rule).
pub fn is_phrasing_only(tag: &str) -> bool {
    matches!(
        tag,
        "p" | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "span"
            | "a"
            | "em"
            | "strong"
            | "small"
            | "time"
            | "abbr"
            | "b"
            | "i"
            | "code"
            | "label"
    )
}

fn known_parts(schemas: &crate::parts::Schemas, kind: &str) -> String {
    schemas
        .get(kind)
        .map(|s| s.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", "))
        .unwrap_or_default()
}

fn known_kinds(schemas: &crate::parts::Schemas) -> String {
    schemas.kind_names().join(", ")
}

/// The HTML void elements — the one list; `slots.rs` block counting uses it
/// too.
pub const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
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
            let line = self.line();
            if self.rest().starts_with("<!--") {
                let end = self
                    .rest()
                    .find("-->")
                    .ok_or_else(|| self.err("unterminated comment"))?;
                let text = self.rest()[..end + 3].to_string();
                nodes.push(Node::Text { text, line });
                self.pos += end + 3;
            } else if self.rest().starts_with("<!") {
                // doctype, verbatim
                let end = self
                    .rest()
                    .find('>')
                    .ok_or_else(|| self.err("unterminated <! declaration"))?;
                let text = self.rest()[..end + 1].to_string();
                nodes.push(Node::Text { text, line });
                self.pos += end + 1;
            } else if self.rest().starts_with('<') {
                nodes.push(Node::Element(self.parse_element()?));
            } else {
                let end = self.rest().find('<').unwrap_or(self.rest().len());
                let text = self.rest()[..end].to_string();
                nodes.push(Node::Text { text, line });
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
            no_collapse: false,
            children: Vec::new(),
            void: VOID.contains(&tag.as_str()),
            line,
            inner: 0..0,
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
                    return Err(self.err(&format!("attribute `{name}` must be double-quoted")));
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
                // A directive, consumed like data-fragment: a build hint, not
                // emitted. `<footer data-no-collapse>` renders even when empty.
                "data-no-collapse" => el.no_collapse = true,
                _ if name.starts_with("data-slot-") => {
                    let attr = name["data-slot-".len()..].to_string();
                    let part = value.ok_or_else(|| self.err(&format!("{name} needs a value")))?;
                    el.attrs.push(Attr::Slot(attr, part));
                }
                _ => el.attrs.push(Attr::Literal(name, value)),
            }
        }
        if el.void {
            return Ok(el);
        }
        el.inner.start = self.pos;
        // Raw-text elements: the content is opaque up to the literal close tag.
        if el.tag == "script" || el.tag == "style" {
            let close = format!("</{}>", el.tag);
            let end = self
                .rest()
                .find(&close)
                .ok_or_else(|| self.err(&format!("unterminated <{}>", el.tag)))?;
            el.children.push(Node::Text {
                text: self.rest()[..end].to_string(),
                line: self.line(),
            });
            el.inner.end = self.pos + end;
            self.pos += end + close.len();
            return Ok(el);
        }
        el.children = self.parse_nodes()?;
        el.inner.end = self.pos;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn frags(sources: &[(&str, &str)]) -> Result<Fragments> {
        // Engine kinds only: these fixtures test the binder, not the merge.
        let schemas = crate::parts::Schemas::engine_only();
        Fragments::load(
            sources
                .iter()
                .map(|(k, s)| (k.to_string(), s.to_string(), format!("{k}.html")))
                .collect(),
            &schemas,
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
            "row",
            r#"<article><h2 data-slot="title"></h2><section data-slot="content"></section></article>"#,
        )])
        .unwrap();
        let mut m = PartMap::new("row");
        m.set("title", Part::Text("A <b> & B".into()));
        m.set("content", Part::Html("<p>hi</p>".into()));
        let out = f.render(&m);
        assert_eq!(
            out,
            r#"<article data-kind="row"><h2 data-slot="title">A &lt;b&gt; &amp; B</h2><section data-slot="content"><p>hi</p></section></article>"#
        );
    }

    #[test]
    fn empty_part_deletes_element() {
        let f = frags(&[(
            "row",
            r#"<article><time data-slot="date"></time><section data-slot="content"></section></article>"#,
        )])
        .unwrap();
        let mut m = PartMap::new("row");
        m.set("content", Part::Html("x".into()));
        let out = f.render(&m);
        assert!(!out.contains("<time"), "{out}");
        // And an empty stream deletes its container too.
        let f = frags(&[
            (
                "row",
                r#"<article><div data-slot="tags" data-fragment="tag"></div></article>"#,
            ),
            ("tag", r#"<a data-slot="name"></a>"#),
        ])
        .unwrap();
        let m = PartMap::new("row");
        assert_eq!(f.render(&m), r#"<article data-kind="row"></article>"#);
    }

    #[test]
    fn stream_maps_child_fragment_per_item() {
        let f = frags(&[
            (
                "row",
                r#"<nav data-slot="crumbs" data-fragment="crumb"></nav>"#,
            ),
            (
                "crumb",
                r#"<span><a data-slot-href="url" data-slot="label"></a></span>"#,
            ),
        ])
        .unwrap();
        let mut m = PartMap::new("row");
        m.set(
            "crumbs",
            Part::Stream(vec![crumb("Home", Some("/")), crumb("16", None)]),
        );
        let out = f.render(&m);
        // Linked crumb gets href; the inert tail is a placeholder link —
        // `<a>` with no href, selectable as a:not([href]).
        assert_eq!(
            out,
            r#"<nav data-slot="crumbs" data-kind="row"><span data-kind="crumb"><a href="/" data-slot="label">Home</a></span><span data-kind="crumb"><a data-slot="label">16</a></span></nav>"#
        );
    }

    #[test]
    fn map_part_renders_named_fragment() {
        let f = frags(&[
            (
                "row",
                r#"<div><nav data-slot="pagination" data-fragment="pagination"></nav></div>"#,
            ),
            (
                "pagination",
                r#"<div><a data-slot-href="prev">Prev</a><ol data-slot="pages" data-fragment="page_link"></ol></div>"#,
            ),
            (
                "page_link",
                r#"<li><a data-slot-href="url" data-slot="n"></a></li>"#,
            ),
        ])
        .unwrap();
        let mut m = PartMap::new("row");
        m.set(
            "pagination",
            Part::Map(
                crate::parts::pagination(
                    2,
                    &[
                        "/blog/".into(),
                        "/blog/page/2".into(),
                        "/blog/page/3".into(),
                    ],
                )
                .unwrap(),
            ),
        );
        let out = f.render(&m);
        assert!(out.contains(r#"<a href="/blog/">Prev</a>"#), "{out}");
        assert!(
            out.contains(r#"<li data-kind="page_link"><a data-slot="n">2</a></li>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<a href="/blog/page/3" data-slot="n">3</a>"#),
            "{out}"
        );
    }

    #[test]
    fn facts_stamp_data_attributes_on_root() {
        let f = frags(&[(
            "row",
            r#"<article><section data-slot="content"></section></article>"#,
        )])
        .unwrap();
        let mut m = PartMap::new("row");
        m.set("tree", Part::Flag(true));
        m.set("content", Part::Html("x".into()));
        let out = f.render(&m);
        assert!(
            out.starts_with(r#"<article data-kind="row" data-tree>"#),
            "{out}"
        );
    }

    #[test]
    fn unknown_slot_is_a_load_error_naming_the_knowns() {
        let e = frags(&[("row", r#"<h2 data-slot="titel"></h2>"#)]).unwrap_err();
        let msg = format!("{e}");
        assert!(msg.contains("row.html:1"), "{msg}");
        assert!(msg.contains("unknown slot `titel`"), "{msg}");
        assert!(msg.contains("title") && msg.contains("url"), "{msg}");
        // The message lists known slots from the derived vocabulary.
    }

    /// Themes are partial (§5e step 4): a kind the theme declines to arrange
    /// renders canonically — the null theme is the fallback, not an error.
    #[test]
    fn missing_child_fragment_falls_back_to_canonical() {
        let f = frags(&[("row", r#"<nav data-slot="crumbs"></nav>"#)]).unwrap();
        let mut m = PartMap::new("row");
        m.set(
            "crumbs",
            Part::Stream(vec![crumb("Home", Some("/")), crumb("16", None)]),
        );
        let out = f.render(&m);
        assert!(out.contains(r#"<section data-kind="crumb">"#), "{out}");
        assert!(
            out.contains(r#"<a data-slot="url" href="/">/</a>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<span data-slot="label">16</span>"#),
            "{out}"
        );
    }

    /// And a map whose own kind has no fragment renders canonically wholesale
    /// — a theme with no fragments at all IS the null theme.
    #[test]
    fn missing_root_fragment_is_the_null_theme() {
        let f = frags(&[]).unwrap();
        let mut m = PartMap::new("row");
        m.set("title", Part::Text("T".into()));
        m.set("content", Part::Html("<p>x</p>".into()));
        let out = f.render(&m);
        assert!(out.starts_with(r#"<section data-kind="row">"#), "{out}");
        assert!(
            out.contains(r#"<div data-slot="content"><p>x</p></div>"#),
            "{out}"
        );
    }

    #[test]
    fn fact_slot_and_void_content_slot_are_load_errors() {
        let e = frags(&[("row", r#"<b data-slot="tree"></b>"#)]).unwrap_err();
        assert!(format!("{e}").contains("is a fact"), "{e}");
        let e = frags(&[("row", r#"<img data-slot="title">"#)]).unwrap_err();
        assert!(format!("{e}").contains("void element <img>"), "{e}");
    }

    #[test]
    fn attr_slot_must_name_a_text_part() {
        let e = frags(&[("row", r#"<a data-slot-href="tags">x</a>"#)]).unwrap_err();
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

    /// q24: `row--card.html` binds the `row` schema; the view's
    /// variant picks it, unknown variants fall back to the base fragment.
    #[test]
    fn variants_bind_their_base_kind_and_render_by_choice() {
        let f = frags(&[
            ("row", r#"<article><h2 data-slot="title"></h2></article>"#),
            (
                "row--card",
                r#"<a data-slot-href="url" data-slot="title"></a>"#,
            ),
        ])
        .unwrap();
        let mut m = PartMap::new("row");
        m.set("title", Part::Text("T".into()));
        m.set("url", Part::Text("/x/".into()));
        assert!(f
            .render_with(&m, Some("card"))
            .starts_with(r#"<a href="/x/""#));
        assert!(f.render_with(&m, None).starts_with("<article"));
        assert!(f.render_with(&m, Some("nope")).starts_with("<article"));
    }

    /// data-fragment names a variant for a map/stream child — and being
    /// explicit, it must resolve, to the right kind.
    #[test]
    fn data_fragment_selects_a_variant_and_must_resolve() {
        let f = frags(&[
            (
                "row",
                r#"<nav data-slot="pagination" data-fragment="pagination--compact"></nav>"#,
            ),
            (
                "pagination--compact",
                r#"<a data-slot-href="prev">prev</a>"#,
            ),
            ("pagination", r#"<div></div>"#),
        ])
        .unwrap();
        let mut m = PartMap::new("row");
        let mut p = PartMap::new("pagination");
        p.set("prev", Part::Text("/older/".into()));
        m.set("pagination", Part::Map(p));
        assert!(
            f.render(&m).contains(r#"<a href="/older/""#),
            "{}",
            f.render(&m)
        );

        let e = frags(&[(
            "row",
            r#"<nav data-slot="pagination" data-fragment="nope"></nav>"#,
        )])
        .unwrap_err();
        assert!(format!("{e}").contains("names no fragment"), "{e}");

        let e = frags(&[
            (
                "row",
                r#"<nav data-slot="pagination" data-fragment="crumb"></nav>"#,
            ),
            ("crumb", r#"<span data-slot="label"></span>"#),
            ("pagination", r#"<div></div>"#),
        ])
        .unwrap_err();
        assert!(format!("{e}").contains("binds kind"), "{e}");
    }

    #[test]
    fn inline_fragment_defines_a_child_when_no_file() {
        let f = Fragments::parse(vec![(
            "row".into(),
            r#"<nav data-slot="crumbs" data-fragment="crumb"><span><a data-slot-href="url" data-slot="label"></a></span></nav>"#.into(),
            "row.html".into(),
        )])
        .unwrap();
        assert!(f.has("crumb"), "inline registered crumb");
        let mut m = PartMap::new("row");
        m.set(
            "crumbs",
            Part::Stream(vec![crumb("Home", Some("/")), crumb("Here", None)]),
        );
        let out = f.render(&m);
        assert!(out.contains(r#"href="/""#), "{out}");
        assert!(out.contains("Home"), "{out}");
        assert!(out.contains("Here"), "{out}");
    }

    #[test]
    fn file_fragment_wins_over_inline() {
        let f = Fragments::parse(vec![
            (
                "row".into(),
                r#"<nav data-slot="crumbs" data-fragment="crumb"><span data-slot="label">INLINE</span></nav>"#.into(),
                "row.html".into(),
            ),
            (
                "crumb".into(),
                r#"<span><a data-slot="label"></a></span>"#.into(),
                "crumb.html".into(),
            ),
        ])
        .unwrap();
        let mut m = PartMap::new("row");
        m.set("crumbs", Part::Stream(vec![crumb("File", None)]));
        let out = f.render(&m);
        assert!(out.contains("File"), "{out}");
        assert!(!out.contains("INLINE"), "file must win: {out}");
    }

    #[test]
    fn whitespace_only_hole_does_not_register_a_fragment() {
        let f = Fragments::parse(vec![(
            "row".into(),
            "<nav data-slot=\"crumbs\" data-fragment=\"crumb\">\n\t  \n</nav>".into(),
            "row.html".into(),
        )])
        .unwrap();
        assert!(!f.has("crumb"), "whitespace is not an inline body");
    }

    #[test]
    fn overlay_keeps_base_inlines_when_parent_is_replaced() {
        let mut f = Fragments::parse(vec![(
            "row".into(),
            r#"<nav data-slot="crumbs" data-fragment="crumb"><span data-slot="label"></span></nav>"#.into(),
            "row.html".into(),
        )])
        .unwrap();
        assert!(f.has("crumb"));
        f.overlay(vec![(
            "row".into(),
            r#"<nav data-slot="crumbs" data-fragment="crumb"></nav>"#.into(),
            "theme/row.html".into(),
        )])
        .unwrap();
        assert!(f.has("crumb"), "base inline must survive parent overlay");
        let schemas = crate::parts::Schemas::engine_only();
        f.validate_against(&schemas).unwrap();
    }

    #[test]
    fn overlay_inline_replaces_base_inline() {
        let mut f = Fragments::parse(vec![(
            "row".into(),
            r#"<nav data-slot="crumbs" data-fragment="crumb"><span data-slot="label">BASE</span></nav>"#.into(),
            "row.html".into(),
        )])
        .unwrap();
        f.overlay(vec![(
            "row".into(),
            r#"<nav data-slot="crumbs" data-fragment="crumb"><span class="crumb" data-slot="label"></span></nav>"#.into(),
            "theme/row.html".into(),
        )])
        .unwrap();
        let mut m = PartMap::new("row");
        m.set("crumbs", Part::Stream(vec![crumb("Here", None)]));
        let out = f.render(&m);
        assert!(
            out.contains(r#"class="crumb""#),
            "theme inline must win: {out}"
        );
        assert!(!out.contains("BASE"), "{out}");
    }

    #[test]
    fn unknown_slot_inside_inline_fails_against_child_kind() {
        // Without extract, `titel` would be checked as a slot on `row`.
        // After extract it must fail against the child kind `crumb`.
        let e = frags(&[(
            "row",
            r#"<nav data-slot="crumbs" data-fragment="crumb"><span data-slot="titel"></span></nav>"#,
        )])
        .unwrap_err()
        .to_string();
        assert!(e.contains("unknown slot `titel`"), "{e}");
        assert!(e.contains("`crumb`"), "{e}");
    }
}

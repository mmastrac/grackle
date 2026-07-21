//! §5e: a layout kind emits a **part map**, not a page.
//!
//! A part is a named, typed value — a scalar, a trusted HTML fragment, a
//! stream of child maps, or a fact. Layout kinds *produce* parts; arranging
//! them into markup belongs to a theme's fragments through the binder
//! (`binder.rs`, `theme.rs`).
//!
//! Two disciplines live here:
//!
//! - **Names are checked against a per-kind schema** (`schema()`), the same
//!   load-time discipline as the filter language (§5). `set()` on an unknown
//!   name is a bug, not a rendering choice.
//! - **Insertion order is canonical semantic order** — reading order, what a
//!   screen reader or the null theme sees. The map is an ordered list, not a
//!   hash, because the order *is* part of the contract.
//!
//! Producers never see `Site` — URLs in parts are root-relative, and prefixing
//! `baseurl` is presentation. Presence is schema-driven: a row with tags gets a
//! `tags` stream, a draft gets a drafts trail; the producer computes, the
//! composer/theme selects (§5a's law).

use crate::db::{Row, SiteDb};

// ------------------------------------------------------------------- model

#[derive(Debug)]
pub enum Part {
    /// A scalar, escaped at fill time.
    Text(String),
    /// A trusted, already-rendered HTML fragment (a row's body).
    Html(String),
    /// Child maps, mapped over a fragment by whoever arranges them.
    Stream(Vec<PartMap>),
    /// A single nested component (pagination on a listing).
    Map(PartMap),
    /// A fact. Facts become `data-` attributes under the theme contract; the
    /// legacy composer branches on them where the old markup did.
    Flag(bool),
}

/// Named parts in canonical order, tagged with the layout kind that filled it.
#[derive(Debug)]
pub struct PartMap {
    pub kind: &'static str,
    parts: Vec<(&'static str, Part)>,
}

impl PartMap {
    pub fn new(kind: &'static str) -> Self {
        debug_assert!(schema(kind).is_some(), "unknown part-map kind `{kind}`");
        PartMap {
            kind,
            parts: Vec::new(),
        }
    }

    pub fn set(&mut self, name: &'static str, part: Part) {
        let ty = part_type(self.kind, name);
        debug_assert!(
            ty.is_some(),
            "part `{name}` is not in the `{}` schema",
            self.kind
        );
        debug_assert!(
            match (&part, ty) {
                (_, None) => true, // the name assert above already fired
                (Part::Text(_), Some(PartType::Text | PartType::Url)) => true,
                (Part::Html(_), Some(PartType::Html)) => true,
                (Part::Stream(v), Some(PartType::Stream(k))) => v.iter().all(|m| m.kind == k),
                (Part::Map(m), Some(PartType::Map(k))) => m.kind == k,
                (Part::Flag(_), Some(PartType::Flag)) => true,
                _ => false,
            },
            "part `{name}` on `{}` does not match its declared type",
            self.kind
        );
        debug_assert!(
            self.parts.iter().all(|(n, _)| *n != name),
            "part `{name}` set twice on `{}`",
            self.kind
        );
        self.parts.push((name, part));
    }

    // The map's read API: the binder fills holes through `get`, `canonical`
    // walks `iter`; the typed accessors below mostly serve tests.
    pub fn get(&self, name: &str) -> Option<&Part> {
        self.parts.iter().find(|(n, _)| *n == name).map(|(_, p)| p)
    }

    #[allow(dead_code)]
    pub fn text(&self, name: &str) -> Option<&str> {
        match self.get(name) {
            Some(Part::Text(s)) => Some(s),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn html(&self, name: &str) -> Option<&str> {
        match self.get(name) {
            Some(Part::Html(s)) => Some(s),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn stream(&self, name: &str) -> &[PartMap] {
        match self.get(name) {
            Some(Part::Stream(v)) => v,
            _ => &[],
        }
    }

    #[allow(dead_code)]
    pub fn map(&self, name: &str) -> Option<&PartMap> {
        match self.get(name) {
            Some(Part::Map(m)) => Some(m),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn flag(&self, name: &str) -> bool {
        matches!(self.get(name), Some(Part::Flag(true)))
    }

    /// Parts in canonical order — what the null theme renders. Unused until
    /// the binder (§5e step 2) walks maps; the order discipline starts now.
    #[allow(dead_code)]
    pub fn iter(&self) -> impl Iterator<Item = (&'static str, &Part)> {
        self.parts.iter().map(|(n, p)| (*n, p))
    }
}

/// The declared type of a part: what the binder checks fragment holes
/// against. `Stream`/`Map` carry the kind of their child maps, so a
/// `data-fragment` reference is type-checked too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartType {
    Text,
    /// A scalar that is a link target. Carried as `Part::Text`; the type
    /// exists so the null theme can render real links and the binder can
    /// insist attribute holes (`data-slot-href`) name something url-shaped.
    Url,
    Html,
    Stream(&'static str),
    Map(&'static str),
    Flag,
}

/// The part schema of each layout kind: which names exist and what fills
/// them. This is what the binder validates fragment holes against (§5e);
/// `set()` also asserts against it so the vocabulary can't drift silently.
pub fn schema(kind: &str) -> Option<&'static [(&'static str, PartType)]> {
    use PartType::*;
    Some(match kind {
        // The outer skeleton. `head` is the computed head facts (§5a);
        // `nav`/`copyright` are site identity, filled from `.slots/` so no
        // theme owns the words; `main` is the rendered layout kind.
        // BODY chrome only (§5g): the engine owns the root HTML shell
        // (doctype/<html>/<head>/<body>) that every theme inherits — a
        // theme's shell.html is the chrome inside <body>. `head` and the
        // subtheme attribute live on the engine root, not here.
        "shell" => &[
            ("nav", Html),
            ("site_title", Text),
            ("main", Html),
            ("copyright", Html),
        ],
        // One row, full content. `tree` is the fact that the row lives in the
        // tree (ancestor crumbs) rather than the dated stream — §5e's
        // `data-tree`: the theme's CSS picks the arrangement.
        "document" => &[
            ("title", Text),
            ("url", Url),
            ("tree", Flag),
            ("crumbs", Stream("crumb")),
            ("tags", Stream("tag")),
            // The row's hero image (q23): an image-typed schema field,
            // thumbnailed, dimension facts attached. The book page's cover.
            ("hero", Map("figure")),
            // §6e's path axis: the enclosing `.section` unit's page tree,
            // with this row marked current. Absent outside sections.
            ("section", Stream("outline_entry")),
            // §6e's heading axis: this document's own outline (`toc:`).
            ("outline", Stream("outline_entry")),
            ("content", Html),
            ("relations", Stream("relation")),
        ],
        // §6e: the ONE recursive kind — hierarchy on either axis (headings
        // or paths) renders through it. An entry with no `url` is an
        // index-less directory's unlinked label (q27); `current` carries
        // the literal `aria-current` value, the pagination trick.
        "outline_entry" => &[
            ("label", Text),
            ("url", Url),
            ("current", Text),
            ("children", Stream("outline_entry")),
        ],
        // N rows, previewed; the view supplied query, filter and title.
        // `featured` is the first row shown large (the book-of-the-month
        // shape); most listings never fill it.
        "listing" => &[
            ("title", Text),
            ("crumbs", Stream("crumb")),
            // q45 mode A: the landing's declared prose, rendered markdown.
            ("intro", Html),
            ("featured", Map("summary")),
            ("items", Stream("summary")),
            ("pagination", Map("pagination")),
        ],
        // ONE preview kind (q36): a summary and a card are the same thing —
        // a view's projection of a row — differing only in what the row HAS
        // (posts: dates/tags/content blocks; books: a hero and a note).
        // Presence is schema-driven, §5a's document argument one level down.
        // `truncated`: the content is a prefix of the document (§6d) — the
        // fact the theme gates the ★ on, stamped as `data-truncated`.
        "summary" => &[
            ("title", Text),
            ("url", Url),
            ("date", Text),
            ("date_pretty", Text),
            ("src", Url),
            ("width", Text),
            ("height", Text),
            ("note", Text),
            ("truncated", Flag),
            ("tags", Stream("tag")),
            ("content", Html),
        ],
        // N object rows as pictures (§5 audit: the gallery archetype). Each
        // figure carries q26's dimension facts so the browser can reserve
        // space (masonry without layout shift); `url` links the original,
        // `src` is the thumbnail (§6b).
        "gallery" => &[
            ("title", Text),
            ("crumbs", Stream("crumb")),
            ("intro", Html),
            ("items", Stream("figure")),
        ],
        "figure" => &[
            ("url", Url),
            ("src", Url),
            ("width", Text),
            ("height", Text),
            ("alt", Text),
        ],
        // N rows as bare titled links (`/`'s embedded latest-posts block).
        "link_list" => &[("items", Stream("link"))],
        "link" => &[("title", Text), ("url", Url)],
        // A crumb with no `url` is the trail's inert tail.
        "crumb" => &[("label", Text), ("url", Url)],
        "tag" => &[("name", Text), ("url", Url)],
        // A post relates to others along AXES — embedding similarity,
        // earlier, later, and whatever comes next (same-tag, series). Each
        // axis is one relation group; the post pivots along all of them.
        // The axis rides as an attribute hole (`data-axis`) for CSS.
        "relation" => &[
            ("axis", Text),
            ("label", Text),
            ("items", Stream("neighbor")),
        ],
        "neighbor" => &[
            ("url", Url),
            ("date", Text),
            ("date_pretty", Text),
            ("title", Text),
        ],
        // `prev`/`next` are absent at the ends of the range; a page with no
        // `url` is the current page, and `current` carries the literal
        // `aria-current` value ("page") so the fragment's attribute hole
        // emits it only there — a11y and the CSS gap trick from one part.
        "pagination" => &[("prev", Url), ("next", Url), ("pages", Stream("page_link"))],
        "page_link" => &[("n", Text), ("url", Url), ("current", Text)],
        // The row's content *is* main (§5a).
        "raw" => &[("content", Html)],
        _ => return None,
    })
}

/// Look up one part's declared type.
pub fn part_type(kind: &str, name: &str) -> Option<PartType> {
    schema(kind)?
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, t)| *t)
}

// --------------------------------------------------------------- canonical

/// The null theme (§5e step 4): a part map rendered with **no fragments at
/// all** — canonical order, generic semantic markup, derived purely from the
/// part types. This is what a theme's absence looks like, the fallback for
/// any kind a theme declines to arrange, and the falsifier: if the canonical
/// render of a row is not complete, the parts layer dropped something, and
/// no fragment can put it back.
///
/// The vocabulary is deliberately tiny: the kind root is a `<section
/// data-kind>` stamped with its facts, scalars are `<span data-slot>`, urls
/// are real links, trusted HTML and nested maps get `<div data-slot>`.
/// Element *choice* beyond that (headings, time elements) is a theme
/// decision, which is exactly what the null theme doesn't make.
pub fn canonical(m: &PartMap) -> String {
    let mut out = String::new();
    canonical_into(m, &mut out);
    out
}

fn canonical_into(m: &PartMap, out: &mut String) {
    use std::fmt::Write as _;
    let _ = write!(out, "<section data-kind=\"{}\"", m.kind);
    for (n, p) in m.iter() {
        if matches!(p, Part::Flag(true)) {
            let _ = write!(out, " data-{n}");
        }
    }
    out.push_str(">\n");
    for (n, p) in m.iter() {
        match p {
            Part::Text(v) => {
                if part_type(m.kind, n) == Some(PartType::Url) {
                    let e = crate::render::esc(v);
                    let _ = write!(out, "<a data-slot=\"{n}\" href=\"{e}\">{e}</a>\n");
                } else {
                    let _ = write!(
                        out,
                        "<span data-slot=\"{n}\">{}</span>\n",
                        crate::render::esc(v)
                    );
                }
            }
            Part::Html(v) => {
                let _ = write!(out, "<div data-slot=\"{n}\">{v}</div>\n");
            }
            Part::Stream(items) => {
                let _ = write!(out, "<div data-slot=\"{n}\">\n");
                for item in items {
                    canonical_into(item, out);
                }
                out.push_str("</div>\n");
            }
            Part::Map(sub) => {
                let _ = write!(out, "<div data-slot=\"{n}\">\n");
                canonical_into(sub, out);
                out.push_str("</div>\n");
            }
            Part::Flag(_) => {}
        }
    }
    out.push_str("</section>\n");
}

// --------------------------------------------------------------- producers

fn crumb(label: String, url: Option<String>) -> PartMap {
    let mut c = PartMap::new("crumb");
    c.set("label", Part::Text(label));
    if let Some(u) = url {
        c.set("url", Part::Text(u));
    }
    c
}

/// Crumb trails arrive as `(label, url?)` pairs — a crumb with no url is the
/// trail's inert tail. The *derivation* lives with the caller: post and
/// listing trails are provenance walks over the view chain (§5c), which
/// needs the config; tree trails are the ancestor axis. Producers here just
/// give the data its shape.
pub fn crumb_stream(trail: Vec<(String, Option<String>)>) -> Part {
    Part::Stream(trail.into_iter().map(|(l, u)| crumb(l, u)).collect())
}

fn tag_stream(cfg: &crate::config::Config, p: &Row) -> Option<Part> {
    if p.tags.is_empty() {
        return None;
    }
    let v = p
        .tags
        .iter()
        .map(|t| {
            // Tag records: display name follows the row's locale (§6f);
            // the URL comes from the OWNING VIEW's route template (q32
            // settled — config can move the archive and pills follow).
            // A site with no tag archive gets unlinked pills.
            let mut m = PartMap::new("tag");
            m.set("name", Part::Text(cfg.tag_name(t, &p.locale).to_string()));
            if let Some(url) = cfg.tag_url(t, &p.locale) {
                m.set("url", Part::Text(url));
            }
            m
        })
        .collect();
    Some(Part::Stream(v))
}

/// The relations axes (§6b). Each is a traversal of an index: `Similar`
/// ranks a metric, `Later`/`Earlier` step the chronological ordering,
/// `LinkedFrom` follows the link graph, and `Translations` pivots
/// `by_logical` — hold the logical path fixed, vary the locale (§6f).
///
/// Closed and typed because the axis string is a THEME CONTRACT: themes
/// key CSS on `[data-axis]`, so a renamed literal breaks them silently.
/// A new axis is a variant plus an `ENGINE_STRINGS` key — no schema
/// change, and the `relation` fragment renders axes it has never heard of.
#[derive(Clone, Copy)]
enum Axis {
    Translations,
    Similar,
    LinkedFrom,
    Later,
    Earlier,
}

impl Axis {
    /// The `data-axis` value themes style on.
    fn as_str(self) -> &'static str {
        match self {
            Axis::Translations => "translations",
            Axis::Similar => "similar",
            // The one axis hyphenated on the wire but underscored as a
            // vocabulary key.
            Axis::LinkedFrom => "linked-from",
            Axis::Later => "later",
            Axis::Earlier => "earlier",
        }
    }

    /// The `ENGINE_STRINGS` key naming this axis (§6f), resolved per locale.
    fn string_key(self) -> &'static str {
        match self {
            Axis::Translations => "translations",
            Axis::Similar => "related",
            Axis::LinkedFrom => "linked_from",
            Axis::Later => "later",
            Axis::Earlier => "earlier",
        }
    }
}

/// One neighbor: the shape every axis yields, dated or not.
fn neighbor(title: &str, url: &str, date: Option<chrono::NaiveDate>) -> PartMap {
    let mut nm = PartMap::new("neighbor");
    nm.set("url", Part::Text(url.to_string()));
    if let Some(d) = date {
        nm.set("date", Part::Text(crate::db::iso_date(d)));
        nm.set("date_pretty", Part::Text(crate::db::pretty_date(d)));
    }
    nm.set("title", Part::Text(title.to_string()));
    nm
}

/// One relations group. An axis with nothing to say contributes no group
/// (rule 2), so every caller can push unconditionally.
fn relation(
    cfg: &crate::config::Config,
    locale: &str,
    axis: Axis,
    items: Vec<PartMap>,
) -> Option<PartMap> {
    if items.is_empty() {
        return None;
    }
    let mut g = PartMap::new("relation");
    g.set("axis", Part::Text(axis.as_str().into()));
    g.set(
        "label",
        Part::Text(cfg.i18n.string(axis.string_key(), locale).to_string()),
    );
    g.set("items", Part::Stream(items));
    Some(g)
}

/// The `translations` group (§6f): this row in other locales, as dateless
/// neighbors labelled by language.
///
/// Takes `(locale, url)` and names the locale here, because the LOCALE is
/// what the head needs for `hreflang` (q53) and the name is only wanted
/// for display. One computation, two consumers.
pub fn translations_group(
    cfg: &crate::config::Config,
    locale: &str,
    translations: &[(String, String)],
) -> Option<PartMap> {
    let items = translations
        .iter()
        .map(|(loc, url)| neighbor(cfg.i18n.name_of(loc), url, None))
        .collect();
    relation(cfg, locale, Axis::Translations, items)
}

/// The `linked-from` group (q38): rows that link here. The link graph's
/// first face — one more axis, the §6b design absorbing its first
/// non-similarity member. Dated where the citing row has a date, which is
/// most of them: a post citing this one says *when*, and that is what
/// makes the list readable; a citing PAGE has none, and the theme lets an
/// undated neighbour span.
fn linked_from_group(
    cfg: &crate::config::Config,
    locale: &str,
    backlinks: &[(String, String, Option<chrono::NaiveDate>)],
) -> Option<PartMap> {
    let items = backlinks
        .iter()
        .map(|(title, url, date)| neighbor(title, url, *date))
        .collect();
    relation(cfg, locale, Axis::LinkedFrom, items)
}

/// One dated row, full content: the `document` kind for a post. Temporal
/// relations (crumb trail, neighbors) are present because the schema has a
/// date, not because anything asked "am I a post".
pub fn document(
    cfg: &crate::config::Config,
    db: &SiteDb,
    p: &Row,
    content: &str,
    trail: Vec<(String, Option<String>)>,
    related: &[crate::db::Key],
    backlinks: &[(String, String, Option<chrono::NaiveDate>)],
    outline: Vec<PartMap>,
    translations: &[(String, String)],
) -> PartMap {
    let mut m = PartMap::new("document");
    m.set("title", Part::Text(p.title.clone().unwrap_or_default()));
    m.set("url", Part::Text(p.url.clone()));
    m.set("crumbs", crumb_stream(trail));
    if let Some(t) = tag_stream(cfg, p) {
        m.set("tags", t);
    }
    if !outline.is_empty() {
        m.set("outline", Part::Stream(outline));
    }
    m.set("content", Part::Html(content.to_string()));
    let row_neighbor = |k: &crate::db::Key| -> Option<PartMap> {
        let n = db.rows.get(k)?;
        Some(neighbor(
            n.title.as_deref().unwrap_or_default(),
            &n.url,
            n.date,
        ))
    };
    // Push order is render order.
    let mut relations = Vec::new();
    relations.extend(translations_group(cfg, &p.locale, translations));
    let similar: Vec<PartMap> = related.iter().filter_map(row_neighbor).collect();
    relations.extend(relation(cfg, &p.locale, Axis::Similar, similar));
    relations.extend(linked_from_group(cfg, &p.locale, backlinks));
    if db.by_url.contains_key(&p.url) {
        // One traversal read in two directions, over the row's collection's
        // declared sequence (q51) rather than the whole table's index.
        let seq = db
            .adjacency
            .get(&p.collection)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let (newer, older) = crate::db::neighbors_in(seq, &p.key);
        for (axis, ix) in [(Axis::Later, newer), (Axis::Earlier, older)] {
            let items = ix.as_ref().and_then(&row_neighbor).into_iter().collect();
            relations.extend(relation(cfg, &p.locale, axis, items));
        }
    }
    if !relations.is_empty() {
        m.set("relations", Part::Stream(relations));
    }
    m
}

/// One tree row, full content: the same `document` kind — the relations differ
/// because the *schema* differs (§5a). Ancestors instead of a date trail, the
/// `tree` fact instead of temporal neighbors, and — inside a `.section` unit
/// (§6e) — the section's page tree with this row marked current.
/// Everything a tree document carries besides its identity — the positional
/// list outgrew itself when §6e and q23 landed.
#[derive(Default)]
pub struct TreeDoc<'a> {
    pub ancestors: &'a [(String, String)],
    pub section: Vec<PartMap>,
    pub outline: Vec<PartMap>,
    pub hero: Option<PartMap>,
    /// Pages that link here (q38) — `(title, url)`.
    pub backlinks: &'a [(String, String, Option<chrono::NaiveDate>)],
    /// This row in other locales (§6f) — `(language label, url)`.
    pub translations: &'a [(String, String)],
}

pub fn document_tree(
    cfg: &crate::config::Config,
    locale: &str,
    home: &str,
    title: &str,
    url: &str,
    d: TreeDoc,
    content: &str,
) -> PartMap {
    let mut m = PartMap::new("document");
    m.set("title", Part::Text(title.to_string()));
    m.set("url", Part::Text(url.to_string()));
    m.set("tree", Part::Flag(true));
    let mut v = vec![(
        cfg.i18n.string("home", locale).to_string(),
        Some(home.to_string()),
    )];
    for (u, t) in d.ancestors {
        v.push((t.clone(), Some(u.clone())));
    }
    v.push((title.to_string(), None));
    m.set("crumbs", crumb_stream(v));
    if let Some(h) = d.hero {
        m.set("hero", Part::Map(h));
    }
    if !d.section.is_empty() {
        m.set("section", Part::Stream(d.section));
    }
    if !d.outline.is_empty() {
        m.set("outline", Part::Stream(d.outline));
    }
    m.set("content", Part::Html(content.to_string()));
    let mut relations = Vec::new();
    relations.extend(translations_group(cfg, locale, d.translations));
    relations.extend(linked_from_group(cfg, locale, d.backlinks));
    if !relations.is_empty() {
        m.set("relations", Part::Stream(relations));
    }
    m
}

fn summary(cfg: &crate::config::Config, p: &Row, content: &str, truncated: bool) -> PartMap {
    let mut m = PartMap::new("summary");
    m.set("title", Part::Text(p.title.clone().unwrap_or_default()));
    m.set("url", Part::Text(p.url.clone()));
    if let Some(d) = p.date {
        m.set("date", Part::Text(crate::db::iso_date(d)));
        m.set("date_pretty", Part::Text(crate::db::pretty_date(d)));
    }
    if truncated {
        m.set("truncated", Part::Flag(true));
    }
    if let Some(t) = tag_stream(cfg, p) {
        m.set("tags", t);
    }
    m.set("content", Part::Html(content.to_string()));
    m
}

/// N rows, summarised. The trail is the route's provenance chain (§5c),
/// computed by the caller. `intro` is the landing's declared prose (q45
/// mode A), already rendered; the slot collapses when absent.
pub fn listing(
    cfg: &crate::config::Config,
    rows: &[(&Row, String, bool)],
    title: &str,
    trail: Vec<(String, Option<String>)>,
    intro: Option<String>,
    pagination: Option<PartMap>,
) -> PartMap {
    let mut m = PartMap::new("listing");
    m.set("title", Part::Text(title.to_string()));
    m.set("crumbs", crumb_stream(trail));
    if let Some(i) = intro {
        m.set("intro", Part::Html(i));
    }
    m.set(
        "items",
        Part::Stream(
            rows.iter()
                .map(|(p, c, t)| summary(cfg, p, c, *t))
                .collect(),
        ),
    );
    if let Some(p) = pagination {
        m.set("pagination", Part::Map(p));
    }
    m
}

/// The landing's route-aware self-embed (q45 mode B): the same listing
/// map with NO title or crumbs — the claimed row owns the arrangement,
/// so only the rows (and their pagination) render; the empty slots
/// collapse.
pub fn listing_embed(
    cfg: &crate::config::Config,
    rows: &[(&Row, String, bool)],
    pagination: Option<PartMap>,
) -> PartMap {
    let mut m = PartMap::new("listing");
    m.set(
        "items",
        Part::Stream(
            rows.iter()
                .map(|(p, c, t)| summary(cfg, p, c, *t))
                .collect(),
        ),
    );
    if let Some(p) = pagination {
        m.set("pagination", Part::Map(p));
    }
    m
}

/// §5d's one genuine component, as data: prev/next (absent at the ends) and
/// the page range (a page with no `url` is the current one). Page 1 lives at
/// `/blog/`; page N>1 links `/blog/page/N` with no trailing slash, faithful to
/// jekyll-paginate. `None` when there is a single page.
/// q32 settled: producers take URLs. `urls[i]` is page i+1's link target,
/// rendered by build from the owning view's route templates — this
/// producer no longer knows what a blog is.
pub fn pagination(current: usize, urls: &[String]) -> Option<PartMap> {
    let total = urls.len();
    if total <= 1 {
        return None;
    }
    let path = |n: usize| urls[n - 1].clone();
    let mut m = PartMap::new("pagination");
    if current > 1 {
        m.set("prev", Part::Text(path(current - 1)));
    }
    if current < total {
        m.set("next", Part::Text(path(current + 1)));
    }
    let pages = (1..=total)
        .map(|n| {
            let mut pm = PartMap::new("page_link");
            pm.set("n", Part::Text(n.to_string()));
            if n != current {
                pm.set("url", Part::Text(path(n)));
            } else {
                pm.set("current", Part::Text("page".into()));
            }
            pm
        })
        .collect();
    m.set("pages", Part::Stream(pages));
    Some(m)
}

/// One gallery item: `(original url, thumb src, dimensions, alt)`.
pub struct Figure {
    pub url: String,
    pub src: String,
    pub dims: Option<(u32, u32)>,
    pub alt: String,
}

/// One `figure` map — a picture with q26's dimension facts attached.
pub fn figure(f: &Figure) -> PartMap {
    let mut fm = PartMap::new("figure");
    fm.set("url", Part::Text(f.url.clone()));
    fm.set("src", Part::Text(f.src.clone()));
    if let Some((w, h)) = f.dims {
        fm.set("width", Part::Text(w.to_string()));
        fm.set("height", Part::Text(h.to_string()));
    }
    fm.set("alt", Part::Text(f.alt.clone()));
    fm
}

/// N object rows as pictures. Dimensions ride as attribute holes so the
/// theme's `<img>` gets `width`/`height` and the page never shifts (q26).
pub fn gallery(
    items: &[Figure],
    title: &str,
    trail: Vec<(String, Option<String>)>,
    intro: Option<String>,
) -> PartMap {
    let mut m = PartMap::new("gallery");
    m.set("title", Part::Text(title.to_string()));
    m.set("crumbs", crumb_stream(trail));
    if let Some(i) = intro {
        m.set("intro", Part::Html(i));
    }
    m.set("items", Part::Stream(items.iter().map(figure).collect()));
    m
}

/// The gallery as a landing self-embed (q45 mode B): pictures only.
pub fn gallery_embed(items: &[Figure]) -> PartMap {
    let mut m = PartMap::new("gallery");
    m.set("items", Part::Stream(items.iter().map(figure).collect()));
    m
}

/// A card-shaped row preview (q23/q36): title + link, optionally a hero
/// image (with dimensions) and a one-line note. Produces a `summary` map —
/// there is one preview kind, and this is its picture-first face.
pub struct CardRow {
    pub title: String,
    pub url: String,
    pub src: Option<String>,
    pub dims: Option<(u32, u32)>,
    pub note: Option<String>,
}

pub fn card(c: &CardRow) -> PartMap {
    let mut m = PartMap::new("summary");
    m.set("title", Part::Text(c.title.clone()));
    m.set("url", Part::Text(c.url.clone()));
    if let Some(s) = &c.src {
        m.set("src", Part::Text(s.clone()));
    }
    if let Some((w, h)) = c.dims {
        m.set("width", Part::Text(w.to_string()));
        m.set("height", Part::Text(h.to_string()));
    }
    if let Some(n) = &c.note {
        m.set("note", Part::Text(n.clone()));
    }
    m
}

/// N rows as previews, optionally with the first featured — the
/// book-of-the-month shape when `featured` (q36: card_list was just a
/// listing wearing a costume); a plain card/preview listing otherwise.
pub fn featured_listing(
    rows: &[CardRow],
    featured: bool,
    title: &str,
    trail: Vec<(String, Option<String>)>,
    intro: Option<String>,
) -> PartMap {
    let mut m = PartMap::new("listing");
    m.set("title", Part::Text(title.to_string()));
    m.set("crumbs", crumb_stream(trail));
    if let Some(i) = intro {
        m.set("intro", Part::Html(i));
    }
    set_card_items(&mut m, rows, featured);
    m
}

/// Cards as a landing self-embed (q45 mode B): the rows (and the
/// featured slot, when declared) with no title or crumbs.
pub fn cards_embed(rows: &[CardRow], featured: bool) -> PartMap {
    let mut m = PartMap::new("listing");
    set_card_items(&mut m, rows, featured);
    m
}

fn set_card_items(m: &mut PartMap, rows: &[CardRow], featured: bool) {
    let items: &[CardRow] = if featured {
        if let Some(first) = rows.first() {
            m.set("featured", Part::Map(card(first)));
        }
        rows.get(1..).unwrap_or(&[])
    } else {
        rows
    };
    if !items.is_empty() {
        m.set("items", Part::Stream(items.iter().map(card).collect()));
    }
}

/// N rows as bare titled links — the smallest listing kind. Items are
/// `(title, url)`, so posts and pages embed alike.
pub fn link_list(items: &[(String, String)]) -> PartMap {
    let mut m = PartMap::new("link_list");
    let v = items
        .iter()
        .map(|(title, url)| {
            let mut lm = PartMap::new("link");
            lm.set("title", Part::Text(title.clone()));
            lm.set("url", Part::Text(url.clone()));
            lm
        })
        .collect();
    m.set("items", Part::Stream(v));
    m
}

/// The row's content *is* `main` — the null theme wraps it, real themes
/// pass it through.
pub fn raw(content: &str) -> PartMap {
    let mut m = PartMap::new("raw");
    m.set("content", Part::Html(content.to_string()));
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crumb_stream_marks_inert_tail_by_missing_url() {
        let s = crumb_stream(vec![("Home".into(), Some("/".into())), ("16".into(), None)]);
        let Part::Stream(v) = s else {
            panic!("not a stream")
        };
        assert_eq!(v[0].text("url"), Some("/"));
        assert_eq!(v[1].text("label"), Some("16"));
        assert_eq!(v[1].text("url"), None);
    }

    #[test]
    fn pagination_is_data_not_markup() {
        assert!(pagination(1, &["/blog/".to_string()]).is_none());
        let urls: Vec<String> = ["/blog/", "/blog/page/2", "/blog/page/3"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let m = pagination(2, &urls).unwrap();
        assert_eq!(m.text("prev"), Some("/blog/"));
        assert_eq!(m.text("next"), Some("/blog/page/3"));
        let pages = m.stream("pages");
        assert_eq!(pages[0].text("url"), Some("/blog/"));
        assert_eq!(pages[1].text("url"), None); // current
        assert_eq!(pages[2].text("url"), Some("/blog/page/3"));
    }

    // Schema conformance is a debug_assert; release builds compile it out.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "not in the `crumb` schema")]
    fn unknown_part_name_asserts() {
        let mut c = PartMap::new("crumb");
        c.set("title", Part::Text("x".into()));
    }

    #[test]
    fn gallery_figures_carry_dimension_facts() {
        let m = gallery(
            &[Figure {
                url: "/photos/a.png".into(),
                src: "/static/x.jpg".into(),
                dims: Some((320, 200)),
                alt: "a".into(),
            }],
            "Photos",
            vec![("Home".into(), Some("/".into()))],
            None,
        );
        let out = canonical(&m);
        assert!(
            out.contains(r#"<a data-slot="src" href="/static/x.jpg">"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<span data-slot="width">320</span>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<span data-slot="height">200</span>"#),
            "{out}"
        );
    }

    #[test]
    fn canonical_renders_order_links_and_facts() {
        let mut m = PartMap::new("document");
        m.set("title", Part::Text("A & B".into()));
        m.set("url", Part::Text("/x/".into()));
        m.set("tree", Part::Flag(true));
        m.set("content", Part::Html("<p>hi</p>".into()));
        let out = canonical(&m);
        assert!(
            out.starts_with(r#"<section data-kind="document" data-tree>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<span data-slot="title">A &amp; B</span>"#),
            "{out}"
        );
        // Url-typed parts are real links — the null theme is navigable.
        assert!(
            out.contains(r#"<a data-slot="url" href="/x/">/x/</a>"#),
            "{out}"
        );
        let t = out.find("data-slot=\"title\"").unwrap();
        let c = out.find("data-slot=\"content\"").unwrap();
        assert!(t < c, "canonical order is insertion order");
    }

    /// The completeness property the null theme exists to falsify: every
    /// part's bytes must survive into the canonical rendering.
    fn complete(m: &PartMap, out: &str) -> bool {
        m.iter().all(|(n, p)| match p {
            Part::Text(v) => out.contains(crate::render::esc(v).as_str()),
            Part::Html(v) => out.contains(v.as_str()),
            Part::Stream(items) => items.iter().all(|c| complete(c, out)),
            Part::Map(sub) => complete(sub, out),
            Part::Flag(true) => out.contains(&format!("data-{n}")),
            Part::Flag(false) => true,
        })
    }

    /// §5e step 4's "run automatically on every row": load the real site and
    /// render every post, page and listing through the null theme, asserting
    /// nothing the parts layer carries is dropped. If a part can vanish, no
    /// fragment can put it back — this catches it at the layer that owns it.
    #[test]
    fn null_theme_is_complete_over_every_real_row() {
        let cfg = crate::config::Config::load(&crate::workspace_root().join("grackle.toml"))
            .expect("grackle.toml loads");
        let db = grackle_source::load(&cfg).expect("site db loads");
        assert!(db.post_ix.len() > 300, "real corpus expected");

        // Keys must actually identify: one per row, resolving back to it.
        // The real corpus is the only place this is worth asserting — a
        // fixture cannot collide two paths that a 27-year site can.
        for r in db.rows.iter() {
            assert_eq!(
                db.row(&r.key).map(|f| &f.rel),
                Some(&r.rel),
                "key {} should resolve to its own row",
                r.key
            );
        }

        // Every post as a full document (raw body stands in for rendered
        // content: completeness is a byte property, not a markdown one).
        for p in db.posts() {
            let trail = vec![
                ("Home".to_string(), Some("/".to_string())),
                (p.title.clone().unwrap_or_default(), None),
            ];
            let body = crate::store::read_body(&p.path).unwrap_or_default();
            let m = document(&cfg, &db, p, &body, trail, &[], &[], Vec::new(), &[]);
            let out = canonical(&m);
            assert!(complete(&m, &out), "post {} dropped a part", p.url);
        }

        // Every routed listing, summaries and pagination included.
        for r in &db.routes {
            if r.view.is_none() || r.members.is_empty() {
                continue;
            }
            let rows: Vec<(&Row, String, bool)> = r
                .members
                .iter()
                .filter_map(|k| db.rows.get(k))
                .enumerate()
                .map(|(i, p)| {
                    let body = crate::store::read_body(&p.path).unwrap_or_default();
                    (p, body, i % 2 == 0)
                })
                .collect();
            let m = listing(
                &cfg,
                &rows,
                r.key.as_deref().unwrap_or("listing"),
                vec![("Home".to_string(), Some("/".to_string()))],
                None,
                r.page.and_then(|n| {
                    let urls: Vec<String> = (1..=66).map(|i| format!("/blog/page/{i}")).collect();
                    pagination(n, &urls)
                }),
            );
            let out = canonical(&m);
            assert!(complete(&m, &out), "listing {} dropped a part", r.url);
        }

        // Every tree page shape (ancestors + title; content is the page's
        // own problem — raw pages bypass parts by design).
        for pg in db.pages() {
            let title = pg.title.clone().unwrap_or_default();
            let m = document_tree(
                &cfg,
                &pg.locale,
                "/",
                &title,
                &pg.url,
                TreeDoc {
                    ancestors: &[("/code/".to_string(), "Code".to_string())],
                    ..Default::default()
                },
                "<p>body</p>",
            );
            let out = canonical(&m);
            assert!(complete(&m, &out), "page {} dropped a part", pg.url);
        }
    }
}

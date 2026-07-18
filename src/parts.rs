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

use crate::db::{Post, SiteDb};

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
        PartMap { kind, parts: Vec::new() }
    }

    pub fn set(&mut self, name: &'static str, part: Part) {
        let ty = part_type(self.kind, name);
        debug_assert!(ty.is_some(), "part `{name}` is not in the `{}` schema", self.kind);
        debug_assert!(
            match (&part, ty) {
                (_, None) => true, // the name assert above already fired
                (Part::Text(_), Some(PartType::Text)) => true,
                (Part::Html(_), Some(PartType::Html)) => true,
                (Part::Stream(v), Some(PartType::Stream(k))) =>
                    v.iter().all(|m| m.kind == k),
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

    // Accessors are the map's read API; the binder pattern-matches `Part`
    // directly, so outside tests these wait for the null theme (step 4).
    #[allow(dead_code)]
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
        "shell" => &[
            ("head", Html),
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
            ("url", Text),
            ("tree", Flag),
            ("crumbs", Stream("crumb")),
            ("tags", Stream("tag")),
            ("content", Html),
            ("neighbors", Stream("neighbor")),
        ],
        // N rows, summarised; the view supplied query, filter and title.
        "listing" => &[
            ("title", Text),
            ("crumbs", Stream("crumb")),
            ("items", Stream("summary")),
            ("pagination", Map("pagination")),
        ],
        "summary" => &[
            ("title", Text),
            ("url", Text),
            ("date", Text),
            ("date_pretty", Text),
            ("tags", Stream("tag")),
            ("content", Html),
        ],
        // N rows as bare titled links (`/`'s embedded latest-posts block).
        "link_list" => &[("items", Stream("link"))],
        "link" => &[("title", Text), ("url", Text)],
        // A crumb with no `url` is the trail's inert tail.
        "crumb" => &[("label", Text), ("url", Text)],
        "tag" => &[("name", Text), ("url", Text)],
        "neighbor" => &[
            ("rel", Text),
            ("label", Text),
            ("url", Text),
            ("date", Text),
            ("date_pretty", Text),
            ("title", Text),
        ],
        // `prev`/`next` are absent at the ends of the range; a page with no
        // `url` is the current page, and `current` carries the literal
        // `aria-current` value ("page") so the fragment's attribute hole
        // emits it only there — a11y and the CSS gap trick from one part.
        "pagination" => &[("prev", Text), ("next", Text), ("pages", Stream("page_link"))],
        "page_link" => &[("n", Text), ("url", Text), ("current", Text)],
        // The row's content *is* main (§5a).
        "raw" => &[("content", Html)],
        _ => return None,
    })
}

/// Look up one part's declared type.
pub fn part_type(kind: &str, name: &str) -> Option<PartType> {
    schema(kind)?.iter().find(|(n, _)| *n == name).map(|(_, t)| *t)
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

fn tag_stream(p: &Post) -> Option<Part> {
    if p.tags.is_empty() {
        return None;
    }
    let v = p
        .tags
        .iter()
        .map(|t| {
            let mut m = PartMap::new("tag");
            m.set("name", Part::Text(t.clone()));
            m.set("url", Part::Text(format!("/blog/tags/{t}/")));
            m
        })
        .collect();
    Some(Part::Stream(v))
}

/// One dated row, full content: the `document` kind for a post. Temporal
/// relations (crumb trail, neighbors) are present because the schema has a
/// date, not because anything asked "am I a post".
pub fn document(
    db: &SiteDb,
    p: &Post,
    content: &str,
    trail: Vec<(String, Option<String>)>,
) -> PartMap {
    let mut m = PartMap::new("document");
    m.set("title", Part::Text(p.title.clone()));
    m.set("url", Part::Text(p.url.clone()));
    m.set("crumbs", crumb_stream(trail));
    if let Some(t) = tag_stream(p) {
        m.set("tags", t);
    }
    m.set("content", Part::Html(content.to_string()));
    if let Some(&i) = db.posts.by_url.get(&p.url) {
        let (newer, older) = db.posts.neighbors(i);
        let item = |rel: &str, label: &str, idx: Option<usize>| -> Option<PartMap> {
            let n = &db.posts.rows[idx?];
            let mut nm = PartMap::new("neighbor");
            nm.set("rel", Part::Text(rel.into()));
            nm.set("label", Part::Text(label.into()));
            nm.set("url", Part::Text(n.url.clone()));
            if let Some(d) = n.date {
                nm.set("date", Part::Text(d.format("%Y-%m-%d").to_string()));
                nm.set("date_pretty", Part::Text(d.format("%-d %B %Y").to_string()));
            }
            nm.set("title", Part::Text(n.title.clone()));
            Some(nm)
        };
        let v: Vec<PartMap> = [
            item("newer", "Later post", newer),
            item("older", "Earlier post", older),
        ]
        .into_iter()
        .flatten()
        .collect();
        m.set("neighbors", Part::Stream(v));
    }
    m
}

/// One tree row, full content: the same `document` kind — the relations differ
/// because the *schema* differs (§5a). Ancestors instead of a date trail, and
/// the `tree` fact instead of temporal neighbors.
pub fn document_tree(
    title: &str,
    url: &str,
    ancestors: &[(String, String)],
    content: &str,
) -> PartMap {
    let mut m = PartMap::new("document");
    m.set("title", Part::Text(title.to_string()));
    m.set("url", Part::Text(url.to_string()));
    m.set("tree", Part::Flag(true));
    let mut v = vec![("Home".to_string(), Some("/".to_string()))];
    for (u, t) in ancestors {
        v.push((t.clone(), Some(u.clone())));
    }
    v.push((title.to_string(), None));
    m.set("crumbs", crumb_stream(v));
    m.set("content", Part::Html(content.to_string()));
    m
}


fn summary(p: &Post, content: &str) -> PartMap {
    let mut m = PartMap::new("summary");
    m.set("title", Part::Text(p.title.clone()));
    m.set("url", Part::Text(p.url.clone()));
    if let Some(d) = p.date {
        m.set("date", Part::Text(d.format("%Y-%m-%d").to_string()));
        m.set("date_pretty", Part::Text(d.format("%-d %B %Y").to_string()));
    }
    if let Some(t) = tag_stream(p) {
        m.set("tags", t);
    }
    m.set("content", Part::Html(content.to_string()));
    m
}

/// N rows, summarised. The trail is the route's provenance chain (§5c),
/// computed by the caller.
pub fn listing(
    rows: &[(&Post, String)],
    title: &str,
    trail: Vec<(String, Option<String>)>,
    pagination: Option<PartMap>,
) -> PartMap {
    let mut m = PartMap::new("listing");
    m.set("title", Part::Text(title.to_string()));
    m.set("crumbs", crumb_stream(trail));
    m.set(
        "items",
        Part::Stream(rows.iter().map(|(p, c)| summary(p, c)).collect()),
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
pub fn pagination(current: usize, total: usize) -> Option<PartMap> {
    if total <= 1 {
        return None;
    }
    let path = |n: usize| {
        if n <= 1 {
            "/blog/".to_string()
        } else {
            format!("/blog/page/{n}")
        }
    };
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

/// N rows as bare titled links — the smallest listing kind.
pub fn link_list(rows: &[&Post]) -> PartMap {
    let mut m = PartMap::new("link_list");
    let v = rows
        .iter()
        .map(|p| {
            let mut lm = PartMap::new("link");
            lm.set("title", Part::Text(p.title.clone()));
            lm.set("url", Part::Text(p.url.clone()));
            lm
        })
        .collect();
    m.set("items", Part::Stream(v));
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crumb_stream_marks_inert_tail_by_missing_url() {
        let s = crumb_stream(vec![
            ("Home".into(), Some("/".into())),
            ("16".into(), None),
        ]);
        let Part::Stream(v) = s else { panic!("not a stream") };
        assert_eq!(v[0].text("url"), Some("/"));
        assert_eq!(v[1].text("label"), Some("16"));
        assert_eq!(v[1].text("url"), None);
    }

    #[test]
    fn pagination_is_data_not_markup() {
        assert!(pagination(1, 1).is_none());
        let m = pagination(2, 3).unwrap();
        assert_eq!(m.text("prev"), Some("/blog/"));
        assert_eq!(m.text("next"), Some("/blog/page/3"));
        let pages = m.stream("pages");
        assert_eq!(pages[0].text("url"), Some("/blog/"));
        assert_eq!(pages[1].text("url"), None); // current
        assert_eq!(pages[2].text("url"), Some("/blog/page/3"));
    }

    #[test]
    #[should_panic(expected = "not in the `crumb` schema")]
    fn unknown_part_name_asserts() {
        let mut c = PartMap::new("crumb");
        c.set("title", Part::Text("x".into()));
    }
}

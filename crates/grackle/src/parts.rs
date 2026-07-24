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
//! - **Schema order is canonical semantic order** — reading order, what a
//!   screen reader or the null theme sees. `set` stores parts at their
//!   `parts.toml` position, not in call order, so the order is the declared
//!   one and a producer cannot drift it.
//!
//! Producers never see `Site` — URLs in parts are root-relative, and prefixing
//! `baseurl` is presentation. Presence is schema-driven: a row with tags gets a
//! `tags` stream, a draft gets a drafts trail; the producer computes, the
//! composer/theme selects (§5a's law).

use crate::db::Row;

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
    /// A fact. Facts become `data-` attributes under the theme contract.
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
        // Stored order follows the SCHEMA, not the call order: `parts.toml` is
        // the canonical reading order the null theme renders (§5e), and a
        // producer must not be able to change it by reordering its `set`s. The
        // sort is stable and the list is tiny; parts a site declared past the
        // engine schema (`set_declared`) sort to the end in the order added.
        if let Some(sch) = schema(self.kind) {
            self.parts
                .sort_by_key(|(n, _)| sch.iter().position(|(sn, _)| sn == n).unwrap_or(usize::MAX));
        }
    }

    /// A map of a kind the engine may not declare — a site's `[[parts]]`
    /// kind, checked against the merged schema by its caller.
    fn new_declared(kind: &'static str) -> Self {
        PartMap {
            kind,
            parts: Vec::new(),
        }
    }

    /// Set a part the engine has no producer for — a site's `[[parts]]`
    /// declaration, filled from a row field. Unlike `set`, the name is not in
    /// the engine schema; `fill_from_fields` has already checked it against
    /// the merged one.
    fn set_declared(&mut self, name: &'static str, part: Part) {
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

    /// Parts in canonical order — what the null theme renders.
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

impl PartType {
    /// As a site spells it in `[[parts]]`, for errors that quote it back.
    pub fn spelling(&self) -> String {
        match self {
            PartType::Text => "text".into(),
            PartType::Url => "url".into(),
            PartType::Html => "html".into(),
            PartType::Flag => "flag".into(),
            PartType::Stream(k) => format!("stream:{k}"),
            PartType::Map(k) => format!("map:{k}"),
        }
    }
}

/// The engine's part schemas, parsed once from `assets/parts.toml`; a site's
/// `[[parts]]` extends them in `Schemas::load`.
///
/// Data, not a match: this is what a theme's fragments are checked against,
/// what `set()` asserts against so the vocabulary cannot drift silently, and
/// the canonical order `set()` stores parts in — so the file is an array of
/// pairs, which preserves order, rather than a table, which would not.
static SCHEMAS: std::sync::OnceLock<Vec<(String, Vec<(String, PartType)>)>> =
    std::sync::OnceLock::new();

#[derive(serde::Deserialize)]
struct KindDecl {
    name: String,
    parts: Vec<(String, String)>,
}

fn parse_part_type(spec: &str, kind: &str, part: &str) -> anyhow::Result<PartType> {
    Ok(match spec {
        "text" => PartType::Text,
        "url" => PartType::Url,
        "html" => PartType::Html,
        "flag" => PartType::Flag,
        _ => match spec.split_once(':') {
            // Leaked deliberately: a child kind name outlives the parse, and
            // the set is closed and tiny. Nothing here is per-request.
            Some(("stream", k)) => PartType::Stream(Box::leak(k.to_string().into_boxed_str())),
            Some(("map", k)) => PartType::Map(Box::leak(k.to_string().into_boxed_str())),
            _ => anyhow::bail!(
                "part `{kind}.{part}`: unknown type {spec:?} — \
                 expected text, url, html, flag, stream:<kind> or map:<kind>"
            ),
        },
    })
}

/// The part vocabulary a build runs against: the engine's kinds, plus
/// whatever `[[parts]]` a site declares.
///
/// A site ADDS. It may declare a new kind, or add parts to an engine kind, but
/// it cannot remove an engine part or change one's type — an engine producer
/// fills those, and a theme that lost `main` would render nothing. Same shape
/// as `[i18n.strings]` (§6f): a closed engine set, extensible, checked at load.
#[derive(Debug)]
pub struct Schemas {
    kinds: Vec<(String, Vec<(&'static str, PartType)>)>,
}

impl Schemas {
    /// The engine's kinds with nothing added — for tests and for the null
    /// theme, which has no site to extend it.
    pub fn engine_only() -> Schemas {
        Schemas { kinds: engine() }
    }

    pub fn load(cfg: &crate::config::Config) -> anyhow::Result<Schemas> {
        let mut kinds = engine();
        for decl in &cfg.parts {
            let mut typed = Vec::new();
            for (name, spec) in &decl.parts {
                // Leaked like the engine's own names: a part name is decided at
                // load and lives as long as the process, and `PartMap` keys are
                // `&'static str` so a declared part is set like any other.
                let leaked: &'static str = Box::leak(name.clone().into_boxed_str());
                typed.push((leaked, parse_part_type(spec, &decl.kind, name)?));
            }
            match kinds.iter_mut().find(|(k, _)| *k == decl.kind) {
                Some((_, existing)) => {
                    for (name, ty) in typed {
                        match existing.iter().find(|(n, _)| *n == name) {
                            Some((_, prev)) if *prev != ty => anyhow::bail!(
                                "part `{}.{name}`: the engine declares it as {prev:?}; a site \
                                 may add parts to an engine kind but not retype one",
                                decl.kind
                            ),
                            Some(_) => {}
                            None => existing.push((name, ty)),
                        }
                    }
                }
                None => kinds.push((decl.kind.clone(), typed)),
            }
        }
        let s = Schemas { kinds };
        s.check_child_kinds()?;
        Ok(s)
    }

    /// Every `stream:`/`map:` must name a kind that exists, or the failure
    /// surfaces later as a fragment that will not bind, far from its cause.
    fn check_child_kinds(&self) -> anyhow::Result<()> {
        for (kind, parts) in &self.kinds {
            for (part, ty) in parts {
                let child = match ty {
                    PartType::Stream(k) | PartType::Map(k) => *k,
                    _ => continue,
                };
                if self.get(child).is_none() {
                    anyhow::bail!(
                        "part `{kind}.{part}` names kind {child:?}, which nothing declares"
                    );
                }
            }
        }
        Ok(())
    }

    pub fn get(&self, kind: &str) -> Option<&[(&'static str, PartType)]> {
        self.kinds
            .iter()
            .find(|(k, _)| k == kind)
            .map(|(_, p)| p.as_slice())
    }

    pub fn kind_names(&self) -> Vec<&str> {
        self.kinds.iter().map(|(k, _)| k.as_str()).collect()
    }
}

/// The engine's kinds as `Schemas` holds them: names borrowed from the
/// process-lifetime asset, so they are already `'static`.
fn engine() -> Vec<(String, Vec<(&'static str, PartType)>)> {
    schemas()
        .iter()
        .map(|(k, parts)| {
            (
                k.clone(),
                parts.iter().map(|(n, t)| (n.as_str(), *t)).collect(),
            )
        })
        .collect()
}

fn schemas() -> &'static [(String, Vec<(String, PartType)>)] {
    SCHEMAS.get_or_init(|| {
        #[derive(serde::Deserialize)]
        struct File {
            kind: Vec<KindDecl>,
        }
        let f: File = toml::from_str(include_str!("../assets/parts.toml"))
            .expect("parts.toml is an engine asset and must parse");
        f.kind
            .into_iter()
            .map(|k| {
                let parts = k
                    .parts
                    .iter()
                    .map(|(n, t)| {
                        (
                            n.clone(),
                            parse_part_type(t, &k.name, n).expect("engine asset"),
                        )
                    })
                    .collect();
                (k.name, parts)
            })
            .collect()
    })
}

/// The parts a kind declares, in canonical order. `None` for an unknown kind,
/// which is what makes a fragment named after one a load error.
pub fn schema(kind: &str) -> Option<&'static [(String, PartType)]> {
    schemas()
        .iter()
        .find(|(k, _)| k == kind)
        .map(|(_, parts)| parts.as_slice())
}

/// Look up one part's declared type.
pub fn part_type(kind: &str, name: &str) -> Option<PartType> {
    schema(kind)?
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, t)| *t)
}

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

pub(crate) fn tag_stream(cfg: &crate::config::Config, p: &Row) -> Option<Part> {
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

/// One neighbor: the shape every relation yields, dated or not.
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

/// One relations group (§6g): a named, labelled list of neighbours. The name
/// is the theme contract — themes key CSS on `[data-relation="…"]` (renamed
/// from `data-axis` at the axis/relation split, q53) — and the `relation`
/// fragment renders names it has never heard of. An empty list contributes
/// no group (hole-algebra rule 2).
fn relation_group(name: &str, label: &str, items: Vec<PartMap>) -> Option<PartMap> {
    if items.is_empty() {
        return None;
    }
    let mut g = PartMap::new("relation");
    g.set("relation", Part::Text(name.to_string()));
    g.set("label", Part::Text(label.to_string()));
    g.set("items", Part::Stream(items));
    Some(g)
}

/// The engine's relation groups (§6g), already evaluated, as parts. Each
/// carries its resolved label; empties are dropped upstream.
pub fn relation_groups(groups: Vec<crate::relate::Group>) -> Vec<PartMap> {
    groups
        .into_iter()
        .filter_map(|g| {
            let items = g
                .items
                .iter()
                .map(|(title, url, date)| neighbor(title, url, *date))
                .collect();
            relation_group(&g.name, &g.label, items)
        })
        .collect()
}

/// The `translations` group (§6f, q53): this row in other locales, as
/// dateless neighbours labelled by language. An *axis* rather than a
/// relation — other forms of THIS row, not other rows — so the engine owns
/// it directly instead of the declared-relation machinery; it still renders
/// through the shared `relation` fragment, stamped `translations`.
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
    relation_group("translations", cfg.i18n.string("translations", locale), items)
}

/// Everything the `document` kind can carry. The two producers below differ
/// in what they can SUPPLY — a dated row has tags and temporal neighbours, a
/// tree row has ancestors and a section — not in how it is laid out, which is
/// §5a's claim made structural. Canonical order lives in `assemble` alone.
#[derive(Default)]
pub struct Document<'a> {
    pub title: String,
    pub url: String,
    pub tree: bool,
    pub crumbs: Vec<(String, Option<String>)>,
    pub hero: Option<PartMap>,
    pub tags: Option<Part>,
    pub section: Vec<PartMap>,
    pub outline: Vec<PartMap>,
    pub content: &'a str,
    pub relations: Vec<PartMap>,
}

fn assemble(d: Document) -> PartMap {
    let mut m = PartMap::new("document");
    m.set("title", Part::Text(d.title));
    m.set("url", Part::Text(d.url));
    if d.tree {
        m.set("tree", Part::Flag(true));
    }
    m.set("crumbs", crumb_stream(d.crumbs));
    if let Some(h) = d.hero {
        m.set("hero", Part::Map(h));
    }
    if let Some(t) = d.tags {
        m.set("tags", t);
    }
    if !d.section.is_empty() {
        m.set("section", Part::Stream(d.section));
    }
    if !d.outline.is_empty() {
        m.set("outline", Part::Stream(d.outline));
    }
    m.set("content", Part::Html(d.content.to_string()));
    if !d.relations.is_empty() {
        m.set("relations", Part::Stream(d.relations));
    }
    m
}

/// One dated row. Its neighbour lists are declared relations now (§6g),
/// evaluated by the engine and handed in as `relation_groups`; the layout
/// just places them, after the `translations` axis. What relations a row has
/// is a fact of its collection's config, not a branch on "am I a post".
pub fn document(
    cfg: &crate::config::Config,
    p: &Row,
    content: &str,
    trail: Vec<(String, Option<String>)>,
    relation_groups: Vec<PartMap>,
    outline: Vec<PartMap>,
    translations: &[(String, String)],
) -> PartMap {
    // Push order is render order: the translations axis first, then the
    // declared relations in the order the loader pinned.
    let mut relations = Vec::new();
    relations.extend(translations_group(cfg, &p.locale, translations));
    relations.extend(relation_groups);
    assemble(Document {
        title: p.title.clone().unwrap_or_default(),
        url: p.url.clone(),
        crumbs: trail,
        tags: tag_stream(cfg, p),
        outline,
        content,
        relations,
        ..Default::default()
    })
}

/// One tree row: the same `document` kind — the relations differ
/// because the *schema* differs (§5a). Ancestors instead of a date trail, the
/// `tree` fact instead of temporal neighbors, and — inside a `.section` unit
/// (§6e) — the section's page tree with this row marked current.
/// Everything a tree document carries besides its identity.
#[derive(Default)]
pub struct TreeDoc<'a> {
    pub ancestors: &'a [(String, String)],
    pub section: Vec<PartMap>,
    pub outline: Vec<PartMap>,
    pub hero: Option<PartMap>,
    /// This tree row's declared relation groups (§6g), pre-evaluated — for a
    /// page that is `linked_from` by default, plus whatever the collection
    /// declares (field-notes' `same_course`).
    pub relation_groups: Vec<PartMap>,
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
    let mut v = vec![(
        cfg.i18n.string("home", locale).to_string(),
        Some(home.to_string()),
    )];
    for (u, t) in d.ancestors {
        v.push((t.clone(), Some(u.clone())));
    }
    v.push((title.to_string(), None));
    let mut relations = Vec::new();
    relations.extend(translations_group(cfg, locale, d.translations));
    relations.extend(d.relation_groups);
    assemble(Document {
        title: title.to_string(),
        url: url.to_string(),
        tree: true,
        crumbs: v,
        hero: d.hero,
        section: d.section,
        outline: d.outline,
        content,
        relations,
        ..Default::default()
    })
}

/// A row as the view sees it, for the one preview projection below.
///
/// Everything optional is *presence*: a field the row cannot answer is not
/// set, and rule 2 of the hole algebra deletes its element. That is what makes
/// one projection serve a post, a book and a photograph — they differ by what
/// they HAVE, which is q36's settlement.
#[derive(Default)]
pub struct Preview<'a> {
    pub row: Option<&'a Row>,
    /// Rendered body, whole or truncated. Absent for a row with no prose.
    pub content: Option<String>,
    pub truncated: bool,
    /// Published thumbnail URL and its measured size (§6b, q26).
    pub src: Option<String>,
    pub dims: Option<(u32, u32)>,
    /// The row's own words, when it is not a `Row` — an object's stem.
    pub title: Option<String>,
    pub url: Option<String>,
    pub note: Option<String>,
    /// Already a `Stream("tag")` — computed by the caller, because tag URLs
    /// come from the owning view's route template rather than the row.
    pub tags: Option<Part>,
}

/// One row, projected into the `summary` kind.
///
/// Fill a kind's declared-but-unproduced parts from the row's typed fields
/// (§5b × §5e).
///
/// The engine produces the parts it knows: title, date, content. A part it
/// does NOT know can only have come from a site's `[[parts]]`, and the row's
/// own `.schema.toml` field of that name is the one thing that could fill it —
/// so a site declares `score = { type = "int" }`, places `data-slot="score"`,
/// and the number appears. Nothing else in the engine changes.
///
/// Types must line up. A mismatch is an error rather than an empty element,
/// because the empty element is exactly the silent failure this closes: a
/// declared part with no filler renders as nothing at all.
pub fn fill_from_fields(
    m: &mut PartMap,
    row: &Row,
    schemas: &Schemas,
    resolve_asset: &dyn Fn(&str) -> String,
) -> anyhow::Result<()> {
    use grackle_db::Value as V;
    let Some(decl) = schemas.get(m.kind) else {
        return Ok(());
    };
    for (name, ty) in decl {
        if m.get(name).is_some() {
            continue; // an engine producer already answered
        }
        let Some(v) = row.fields.get(*name) else {
            continue;
        };
        let part = match (v, ty) {
            (V::Str(s), PartType::Text) => Part::Text(s.clone()),
            // An image-typed field is a reference to an asset (checked at
            // load): `resolve_asset` yields the thumbnail's URL when one
            // exists, else the original under baseurl. A plain string filling
            // a url part is the author's own link, left as written.
            // Presentation stays with the caller — this still sees neither
            // baseurl nor the thumb map.
            (V::Str(s), PartType::Url) => {
                if row.images.contains_key(*name) {
                    Part::Text(resolve_asset(s))
                } else {
                    Part::Text(s.clone())
                }
            }
            (V::Int(n), PartType::Text) => Part::Text(n.to_string()),
            // A fact, not text: `data-<name>` on the fragment root, which is
            // where theme CSS can reach it. False sets nothing, so the theme
            // selects on presence exactly as it does for `truncated`.
            (V::Bool(b), PartType::Flag) => {
                if !*b {
                    continue;
                }
                Part::Flag(true)
            }
            // A list of strings fills a stream whose child kind is that
            // shape: exactly one text part, one child map per string. The
            // engine ships `item`/`label`; a site that wants its own
            // `data-kind` for CSS declares a kind of the same shape.
            (V::List(items), PartType::Stream(child)) => {
                let shape = schemas.get(child).unwrap_or(&[]);
                let label = match shape {
                    [(label, PartType::Text)] => *label,
                    _ => anyhow::bail!(
                        "{}: field `{name}` is a list, so kind `{child}` must declare \
                         exactly one text part to receive it — `{child}` declares {}",
                        row.rel.display(),
                        if shape.is_empty() {
                            "nothing".to_string()
                        } else {
                            shape
                                .iter()
                                .map(|(n, t)| format!("{n}: {}", t.spelling()))
                                .collect::<Vec<_>>()
                                .join(", ")
                        }
                    ),
                };
                Part::Stream(
                    items
                        .iter()
                        .map(|s| {
                            let mut cm = PartMap::new_declared(child);
                            cm.set_declared(label, Part::Text(s.clone()));
                            cm
                        })
                        .collect(),
                )
            }
            (V::Null, _) => continue,
            (v, ty) => anyhow::bail!(
                "{}: field `{name}` is {}, which cannot fill a `{}` part of kind `{}`",
                row.rel.display(),
                v.type_name(),
                ty.spelling(),
                m.kind
            ),
        };
        m.set_declared(name, part);
    }
    Ok(())
}

/// A part is filled when the row answers it — one projection serves a post, a
/// book and a photograph (q36).
pub fn preview(p: Preview) -> PartMap {
    let mut m = PartMap::new("summary");
    let row = p.row;
    let title = p
        .title
        .clone()
        .or_else(|| row.and_then(|r| r.title.clone()))
        .unwrap_or_default();
    m.set("title", Part::Text(title));
    let url = p
        .url
        .clone()
        .or_else(|| row.map(|r| r.url.clone()))
        .unwrap_or_default();
    m.set("url", Part::Text(url));
    if let Some(d) = row.and_then(|r| r.date) {
        m.set("date", Part::Text(crate::db::iso_date(d)));
        m.set("date_pretty", Part::Text(crate::db::pretty_date(d)));
    }
    if let Some(s) = &p.src {
        m.set("src", Part::Text(s.clone()));
    }
    if let Some((w, h)) = p.dims {
        m.set("width", Part::Text(w.to_string()));
        m.set("height", Part::Text(h.to_string()));
    }
    if let Some(n) = p
        .note
        .clone()
        .or_else(|| row.and_then(|r| r.description.clone()))
    {
        m.set("note", Part::Text(n));
    }
    if p.truncated {
        m.set("truncated", Part::Flag(true));
    }
    if let Some(t) = p.tags {
        m.set("tags", t);
    }
    if let Some(c) = &p.content {
        m.set("content", Part::Html(c.clone()));
    }
    m
}

/// N previews, arranged. The trail is the route's provenance chain (§5c),
/// computed by the caller. `intro` is the landing's declared prose (q45
/// mode A), already rendered; the slot collapses when absent. `featured`
/// lifts the first preview into its own slot — the book-of-the-month shape.
///
/// One producer, because there is one arrangement: the blog index, the
/// photo gallery and the book club differ in what their PREVIEWS hold and
/// which fragment the view names, never in this map.
pub fn listing(
    items: Vec<Preview>,
    featured: bool,
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
    set_items(&mut m, items, featured);
    if let Some(p) = pagination {
        m.set("pagination", Part::Map(p));
    }
    m
}

/// The landing's route-aware self-embed (q45 mode B): the same listing
/// map with NO title or crumbs — the claimed row owns the arrangement,
/// so only the items (and their pagination) render; the empty slots
/// collapse.
pub fn listing_embed(items: Vec<Preview>, featured: bool, pagination: Option<PartMap>) -> PartMap {
    let mut m = PartMap::new("listing");
    set_items(&mut m, items, featured);
    if let Some(p) = pagination {
        m.set("pagination", Part::Map(p));
    }
    m
}

fn set_items(m: &mut PartMap, mut items: Vec<Preview>, featured: bool) {
    if featured && !items.is_empty() {
        m.set("featured", Part::Map(preview(items.remove(0))));
    }
    if !items.is_empty() {
        m.set(
            "items",
            Part::Stream(items.into_iter().map(preview).collect()),
        );
    }
}

/// §5d's one genuine component, as data: prev/next (absent at the ends) and
/// the page range (a page with no `url` is the current one). `None` when there
/// is a single page.
///
/// q32: `urls[i]` is page i+1's link target, rendered by build from the owning
/// view's route templates — this producer knows nothing about blogs.
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

    /// A picture-first listing needs no kind of its own: `src` links, and the
    /// measured dimensions ride along (q26).
    #[test]
    fn picture_previews_carry_dimension_facts() {
        let m = listing(
            vec![Preview {
                title: Some("a".into()),
                url: Some("/photos/a.png".into()),
                src: Some("/static/x.jpg".into()),
                dims: Some((320, 200)),
                ..Default::default()
            }],
            false,
            "Photos",
            vec![("Home".into(), Some("/".into()))],
            None,
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
        assert!(t < c, "title precedes content, as the schema declares");
    }

    /// Canonical order is the SCHEMA's, not the call order: a producer that
    /// sets parts in a different sequence still renders in `parts.toml` order,
    /// so reordering `set`s cannot silently change the null theme.
    #[test]
    fn canonical_follows_schema_order_not_call_order() {
        // `document` declares title before content; set them the other way.
        let mut m = PartMap::new("document");
        m.set("content", Part::Html("<p>body</p>".into()));
        m.set("title", Part::Text("T".into()));
        let names: Vec<&str> = m.iter().map(|(n, _)| n).collect();
        assert_eq!(
            names,
            ["title", "content"],
            "stored order is the schema's, whatever the call order"
        );
        let out = canonical(&m);
        assert!(
            out.find("data-slot=\"title\"").unwrap() < out.find("data-slot=\"content\"").unwrap(),
            "and canonical renders that order: {out}"
        );
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
            let m = document(&cfg, p, &body, trail, Vec::new(), Vec::new(), &[]);
            let out = canonical(&m);
            assert!(complete(&m, &out), "post {} dropped a part", p.url);
        }

        // Every routed listing, summaries and pagination included.
        for r in &db.routes {
            if r.view.is_none() || r.members.is_empty() {
                continue;
            }
            let rows: Vec<Preview> = r
                .members
                .iter()
                .filter_map(|k| db.rows.get(k))
                .enumerate()
                .map(|(i, p)| Preview {
                    row: Some(p),
                    content: Some(crate::store::read_body(&p.path).unwrap_or_default()),
                    truncated: i % 2 == 0,
                    tags: tag_stream(&cfg, p),
                    ..Default::default()
                })
                .collect();
            let m = listing(
                rows,
                false,
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

#[cfg(test)]
mod schema_asset_tests {
    use super::*;

    /// Every `stream:`/`map:` names a kind that exists. A dangling child kind
    /// would surface as a fragment failing to bind, far from the cause.
    #[test]
    fn every_child_kind_resolves() {
        for (kind, parts) in schemas() {
            for (part, ty) in parts {
                let child = match ty {
                    PartType::Stream(k) | PartType::Map(k) => *k,
                    _ => continue,
                };
                assert!(
                    schema(child).is_some(),
                    "{kind}.{part} names kind {child:?}, which is not declared"
                );
            }
        }
    }

    /// The vocabulary each kind declares, in canonical order — the order
    /// `set()` stores parts in and `canonical()` renders. Pinned so neither
    /// the vocabulary nor the reading order drifts silently.
    #[test]
    fn each_kind_declares_its_vocabulary() {
        let names = |k| {
            schema(k)
                .unwrap()
                .iter()
                .map(|(n, _)| n.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(names("shell"), ["nav", "site_title", "main", "copyright"]);
        assert_eq!(names("item"), ["label"], "the shape a list field fills");
        assert_eq!(
            names("listing"),
            [
                "title",
                "crumbs",
                "intro",
                "featured",
                "items",
                "pagination"
            ]
        );
    }

    /// A kind nothing declares is `None`, which is what makes a fragment
    /// named after a typo a load error rather than an empty render.
    #[test]
    fn an_undeclared_kind_is_none() {
        assert!(schema("summary").is_some());
        assert!(schema("sumary").is_none());
    }
}

#[cfg(test)]
mod config_schema_tests {
    use super::*;
    use grackle_source::config::Config;

    fn cfg(parts: &str) -> Config {
        Config::from_toml(&format!(
            "root = \".\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nname=\"blog\"\nkind=\"posts\"\nsource=\"_posts\"\n{parts}"
        ))
        .expect("test config parses")
    }

    /// A resolver for tests with no image field: it is never consulted,
    /// so it just echoes. Image-specific tests pass their own.
    fn verbatim(s: &str) -> String {
        s.to_string()
    }

    fn row_with(fields: &[(&str, grackle_db::Value)]) -> Row {
        Row {
            rel: std::path::PathBuf::from("reviews/model-m.md"),
            fields: fields
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
            ..Default::default()
        }
    }

    /// A declared part with no engine producer is filled from the row's
    /// `.schema.toml` field of the same name (§5b × §5e) — the mapping from
    /// schema to display. Without it a site could declare `score`, place it,
    /// and get a silently empty element, because the vocabulary was
    /// extensible and the producer set was not.
    #[test]
    fn a_declared_part_is_filled_from_the_rows_field() {
        use grackle_db::Value as V;
        let c = cfg("[[parts]]\nkind = \"document\"\n\
             parts = [[\"score\", \"text\"], [\"verdict\", \"text\"], [\"pick\", \"flag\"]]\n");
        let schemas = Schemas::load(&c).unwrap();
        let row = row_with(&[
            ("score", V::Int(9)),
            ("verdict", V::Str("Still the best".into())),
            ("pick", V::Bool(true)),
        ]);

        let mut m = PartMap::new("document");
        m.set("title", Part::Text("Model M".into()));
        fill_from_fields(&mut m, &row, &schemas, &verbatim).unwrap();

        assert_eq!(m.text("score"), Some("9"), "an int renders as digits");
        assert_eq!(m.text("verdict"), Some("Still the best"));
        assert!(m.flag("pick"), "a bool becomes a fact, not text");
    }

    /// A `list` field fills a stream of one-text-part maps — the engine ships
    /// `item`/`label` for it. A site may name its own kind of the same shape
    /// instead, which is how a list gets its own `data-kind` for theme CSS.
    #[test]
    fn a_list_field_fills_a_stream_of_items() {
        use grackle_db::Value as V;
        let c = cfg("[[parts]]\nkind = \"document\"\nparts = [[\"kit\", \"stream:item\"]]\n");
        let schemas = Schemas::load(&c).unwrap();
        let row = row_with(&[(
            "kit",
            V::List(vec!["heavy pan".into(), "fine sieve".into()]),
        )]);
        let mut m = PartMap::new("document");
        fill_from_fields(&mut m, &row, &schemas, &verbatim).unwrap();

        let items = m.stream("kit");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].kind, "item");
        assert_eq!(items[0].text("label"), Some("heavy pan"));
        assert_eq!(items[1].text("label"), Some("fine sieve"));

        // A site's own kind of the same shape: its `data-kind` reaches theme
        // CSS, and the part it declares receives the string whatever it is
        // called — the name comes from the kind, not from `item`.
        let c = cfg(
            "[[parts]]\nkind = \"document\"\nparts = [[\"kit\", \"stream:kit_item\"]]\n\
             [[parts]]\nkind = \"kit_item\"\nparts = [[\"name\", \"text\"]]\n",
        );
        let schemas = Schemas::load(&c).unwrap();
        let mut m = PartMap::new("document");
        fill_from_fields(&mut m, &row, &schemas, &verbatim).unwrap();
        let items = m.stream("kit");
        assert_eq!(items[0].kind, "kit_item");
        assert_eq!(items[0].text("name"), Some("heavy pan"));
    }

    /// The child kind must BE that shape. A stream of `tag` (name + url) has
    /// no single place to put a string, and guessing one would be the silent
    /// wrong answer — so it names what the kind actually declares.
    #[test]
    fn a_list_needs_a_one_text_part_child_kind() {
        use grackle_db::Value as V;
        let c = cfg("[[parts]]\nkind = \"document\"\nparts = [[\"kit\", \"stream:tag\"]]\n");
        let schemas = Schemas::load(&c).unwrap();
        let mut m = PartMap::new("document");
        let e = fill_from_fields(
            &mut m,
            &row_with(&[("kit", V::List(vec!["x".into()]))]),
            &schemas,
            &verbatim,
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("exactly one text part"), "{e}");
        assert!(e.contains("name: text, url: url"), "names the shape: {e}");
    }

    /// An image field filling a `url` part is a REFERENCE: the value names an
    /// asset (row.images marks it), and the resolver turns that source path
    /// into the published URL — the thumbnail's on a real build. This is the
    /// one arm that consults the caller's presentation.
    #[test]
    fn an_image_field_resolves_to_its_published_url() {
        use grackle_db::Value as V;
        let c = cfg("[[parts]]\nkind = \"document\"\nparts = [[\"shot\", \"url\"]]\n");
        let schemas = Schemas::load(&c).unwrap();
        let mut row = row_with(&[("shot", V::Str("reviews/model-m.jpg".into()))]);
        row.images
            .insert("shot".into(), "reviews/model-m.jpg".into());

        let resolve = |src: &str| format!("/static/deadbeef-{src}");
        let mut m = PartMap::new("document");
        fill_from_fields(&mut m, &row, &schemas, &resolve).unwrap();
        assert_eq!(m.text("shot"), Some("/static/deadbeef-reviews/model-m.jpg"));
    }

    /// A plain string filling a `url` part is the author's OWN link, not an
    /// asset — it is left verbatim and the resolver is never consulted. The
    /// panicking resolver proves the image arm is entered only for a field
    /// `row.images` marks.
    #[test]
    fn a_plain_string_url_field_is_left_as_written() {
        use grackle_db::Value as V;
        let c = cfg("[[parts]]\nkind = \"document\"\nparts = [[\"homepage\", \"url\"]]\n");
        let schemas = Schemas::load(&c).unwrap();
        let row = row_with(&[("homepage", V::Str("https://example.com".into()))]);
        let resolve = |_: &str| panic!("resolver must not run for a non-image string");
        let mut m = PartMap::new("document");
        fill_from_fields(&mut m, &row, &schemas, &resolve).unwrap();
        assert_eq!(m.text("homepage"), Some("https://example.com"));
    }

    /// Presence, not emptiness: a row that does not carry the field sets no
    /// part, so the hole algebra deletes the element rather than rendering a
    /// blank one. `false` is absence too — a fact is selected on presence.
    #[test]
    fn a_row_without_the_field_fills_nothing() {
        use grackle_db::Value as V;
        let c = cfg(
            "[[parts]]\nkind = \"document\"\nparts = [[\"score\", \"text\"], [\"pick\", \"flag\"]]\n",
        );
        let schemas = Schemas::load(&c).unwrap();

        let mut m = PartMap::new("document");
        fill_from_fields(&mut m, &row_with(&[]), &schemas, &verbatim).unwrap();
        assert_eq!(m.text("score"), None);

        let mut m = PartMap::new("document");
        fill_from_fields(
            &mut m,
            &row_with(&[("pick", V::Bool(false))]),
            &schemas,
            &verbatim,
        )
        .unwrap();
        assert!(!m.flag("pick"), "false is absence, not a false fact");
    }

    /// An engine part is never overwritten by a field of the same name: the
    /// producer computed it, and a row that happens to declare `title` must
    /// not shadow the one the engine derived.
    #[test]
    fn an_engine_part_wins_over_a_field_of_the_same_name() {
        use grackle_db::Value as V;
        let schemas = Schemas::load(&cfg("")).unwrap();
        let mut m = PartMap::new("document");
        m.set("title", Part::Text("computed".into()));
        fill_from_fields(
            &mut m,
            &row_with(&[("title", V::Str("front matter".into()))]),
            &schemas,
            &verbatim,
        )
        .unwrap();
        assert_eq!(m.text("title"), Some("computed"));
        // `get` returns the first match, so a shadowing field would be
        // invisible here and render twice in canonical order. Count instead.
        assert_eq!(
            m.iter().filter(|(n, _)| *n == "title").count(),
            1,
            "the field must not be appended alongside the engine's part"
        );
    }

    /// A type that cannot fill the declared part is an ERROR naming the file,
    /// not an empty element — the silent-drop this whole binding closes. In
    /// particular a string may not fill an `html` part: front matter is not
    /// trusted markup.
    #[test]
    fn a_field_that_cannot_fill_its_part_is_an_error() {
        use grackle_db::Value as V;
        let c = cfg("[[parts]]\nkind = \"document\"\nparts = [[\"score\", \"url\"]]\n");
        let schemas = Schemas::load(&c).unwrap();
        let mut m = PartMap::new("document");
        let e = fill_from_fields(
            &mut m,
            &row_with(&[("score", V::Int(9))]),
            &schemas,
            &verbatim,
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("model-m.md"), "names the file: {e}");
        assert!(e.contains("an int"), "names what it got: {e}");
        assert!(e.contains("`url`"), "names the declared type: {e}");

        let c = cfg("[[parts]]\nkind = \"document\"\nparts = [[\"verdict\", \"html\"]]\n");
        let schemas = Schemas::load(&c).unwrap();
        let mut m = PartMap::new("document");
        assert!(
            fill_from_fields(
                &mut m,
                &row_with(&[("verdict", V::Str("<b>x</b>".into()))]),
                &schemas,
                &verbatim
            )
            .is_err(),
            "front matter must not become trusted markup"
        );
    }

    /// A site may invent a kind. This is the whole point: a theme can then
    /// place it, and the binder checks the name against what the SITE
    /// declared rather than against a Rust match.
    #[test]
    fn a_site_may_declare_a_new_kind() {
        let c = cfg("[[parts]]\nkind = \"recipe_card\"\nparts = [[\"servings\", \"text\"]]\n");
        let s = Schemas::load(&c).unwrap();
        assert!(Schemas::engine_only().get("recipe_card").is_none());
        assert_eq!(s.get("recipe_card").unwrap().first().unwrap().0, "servings");
    }

    /// And may add to an engine kind — an engine producer fills the engine
    /// parts, a theme places the added one.
    #[test]
    fn a_site_may_add_a_part_to_an_engine_kind() {
        let c = cfg("[[parts]]\nkind = \"summary\"\nparts = [[\"cook_time\", \"text\"]]\n");
        let s = Schemas::load(&c).unwrap();
        let names: Vec<&str> = s.get("summary").unwrap().iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"title"), "engine parts survive: {names:?}");
        assert!(names.contains(&"cook_time"), "{names:?}");
    }

    /// But may not RETYPE one: an engine producer fills `title` as text, and
    /// a site that redeclared it as a stream would make that producer wrong.
    #[test]
    fn retyping_an_engine_part_is_a_load_error() {
        let c = cfg("[[parts]]\nkind = \"summary\"\nparts = [[\"title\", \"stream:tag\"]]\n");
        let e = Schemas::load(&c).unwrap_err().to_string();
        assert!(e.contains("may add parts"), "{e}");
        assert!(e.contains("summary.title"), "{e}");
    }

    /// A child kind that nothing declares fails at load, where the cause is,
    /// rather than later as a fragment that will not bind.
    #[test]
    fn a_dangling_child_kind_is_a_load_error() {
        let c = cfg("[[parts]]\nkind = \"box\"\nparts = [[\"items\", \"stream:nope\"]]\n");
        let e = Schemas::load(&c).unwrap_err().to_string();
        assert!(e.contains("nope"), "{e}");
    }

    #[test]
    fn an_unknown_type_names_the_legal_ones() {
        let c = cfg("[[parts]]\nkind = \"box\"\nparts = [[\"x\", \"integer\"]]\n");
        let e = Schemas::load(&c).unwrap_err().to_string();
        assert!(e.contains("stream:<kind>"), "{e}");
    }
}

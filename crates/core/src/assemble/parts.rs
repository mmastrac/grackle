//! Part maps: typed holes a theme's fragments place (THEME.md §5).
//!
//! Schema order is canonical reading order. `set` stores at the schema's
//! position, not call order. Producers never see `Site` — URLs stay
//! root-relative.

use crate::model::Row;
use anyhow::Context;
use std::path::Path;

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
        // Schema order, not call order — null theme reading order.
        if let Some(sch) = schema(self.kind) {
            self.parts
                .sort_by_key(|(n, _)| sch.iter().position(|(sn, _)| sn == n).unwrap_or(usize::MAX));
        }
    }

    /// Site-declared kind (not in the engine schema).
    fn new_declared(kind: &'static str) -> Self {
        PartMap {
            kind,
            parts: Vec::new(),
        }
    }

    /// Part filled from a row field; name already checked against merged schema.
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
    /// For errors that quote a part type back.
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

use PartType::{Flag, Html, Map, Stream, Text, Url};

/// Engine part vocabulary + order (THEME.md §5). Themes extend via their own
/// `.schema.toml`; site `[[parts]]` is gone.
const ENGINE: &[(&str, &[(&str, PartType)])] = &[
    (
        "root",
        &[
            ("nav", Html),
            ("site_title", Text),
            ("axes", Stream("axis")),
            ("content", Html),
            ("copyright", Html),
        ],
    ),
    // One presence-driven kind; faces are fragment variants (THEME.md §2).
    (
        "row",
        &[
            ("title", Text),
            ("url", Url),
            ("tree", Flag),
            ("crumbs", Stream("crumb")),
            ("tags", Stream("tag")),
            ("hero", Map("row")),
            ("section", Stream("outline_entry")),
            ("outline", Stream("outline_entry")),
            ("intro", Html),
            ("content", Html),
            ("pagination", Map("pagination")),
            ("date", Text),
            ("date_pretty", Text),
            ("src", Url),
            ("width", Text),
            ("height", Text),
            ("note", Text),
            ("truncated", Flag),
            ("relations", Stream("relation")),
        ],
    ),
    (
        "outline_entry",
        &[
            ("label", Text),
            ("url", Url),
            ("current", Text),
            ("children", Stream("outline_entry")),
        ],
    ),
    (
        "axis",
        &[
            ("axis", Text),
            ("label", Text),
            ("current", Text),
            ("items", Stream("axis_member")),
        ],
    ),
    (
        "axis_member",
        &[("label", Text), ("url", Url), ("current", Text)],
    ),
    ("crumb", &[("label", Text), ("url", Url)]),
    ("tag", &[("name", Text), ("url", Url)]),
    (
        "relation",
        &[
            ("relation", Text),
            ("label", Text),
            ("items", Stream("neighbor")),
        ],
    ),
    (
        "neighbor",
        &[
            // Same card surface as a listing `row` (via `preview`), under the
            // relation face — themes keep `data-kind="neighbor"`.
            ("url", Url),
            ("title", Text),
            ("date", Text),
            ("date_pretty", Text),
            ("note", Text),
            ("src", Url),
            ("width", Text),
            ("height", Text),
            ("truncated", Flag),
            ("content", Html),
        ],
    ),
    (
        "pagination",
        &[("prev", Url), ("next", Url), ("pages", Stream("page_link"))],
    ),
    ("page_link", &[("n", Text), ("url", Url), ("current", Text)]),
    ("item", &[("label", Text)]),
    ("raw", &[("content", Html)]),
];

#[allow(clippy::type_complexity)]
static SCHEMAS: std::sync::OnceLock<Vec<(String, Vec<(String, PartType)>)>> =
    std::sync::OnceLock::new();

#[derive(Debug, Clone)]
pub struct Schemas {
    kinds: Vec<(String, Vec<(&'static str, PartType)>)>,
}

impl Schemas {
    pub fn engine_only() -> Schemas {
        Schemas { kinds: engine() }
    }

    /// Extend with a theme's `.schema.toml` (THEME.md §5): each field becomes
    /// a part on `row`. May not remove or retype an engine part.
    pub fn extend_theme_dir(&self, theme_dir: &Path) -> anyhow::Result<Schemas> {
        let path = theme_dir.join(".schema.toml");
        if !path.is_file() {
            return Ok(self.clone());
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let table: toml::Table = text
            .parse()
            .with_context(|| format!("parsing {}", path.display()))?;
        let mut kinds = self.kinds.clone();
        for (name, val) in &table {
            let Some(part_ty) = field_value_as_part(val) else {
                continue; // nested tables / unknowns — not part decls
            };
            let leaked: &'static str = Box::leak(name.clone().into_boxed_str());
            for kind in ["row"] {
                let Some((_, parts)) = kinds.iter_mut().find(|(k, _)| k == kind) else {
                    continue;
                };
                match parts.iter().find(|(n, _)| *n == leaked) {
                    Some((_, prev)) if *prev != part_ty => anyhow::bail!(
                        "{}: part `{kind}.{name}` is {:?}; a theme may not retype an engine part",
                        path.display(),
                        prev
                    ),
                    Some(_) => {}
                    None => parts.push((leaked, part_ty)),
                }
            }
        }
        Ok(Schemas { kinds })
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

fn field_value_as_part(val: &toml::Value) -> Option<PartType> {
    let ty = val.get("type")?.as_str()?;
    Some(match ty {
        "string" | "int" => PartType::Text,
        "bool" => PartType::Flag,
        "image" => PartType::Url,
        "list" => PartType::Stream("item"),
        _ => return None,
    })
}

fn engine() -> Vec<(String, Vec<(&'static str, PartType)>)> {
    ENGINE
        .iter()
        .map(|(k, parts)| {
            (
                (*k).to_string(),
                parts.iter().map(|(n, t)| (*n, *t)).collect(),
            )
        })
        .collect()
}

fn schemas() -> &'static [(String, Vec<(String, PartType)>)] {
    SCHEMAS.get_or_init(|| {
        ENGINE
            .iter()
            .map(|(k, parts)| {
                (
                    (*k).to_string(),
                    parts.iter().map(|(n, t)| ((*n).to_string(), *t)).collect(),
                )
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

/// Null theme: schema order, generic markup from part types (THEME.md §5).
pub fn canonical(m: &PartMap) -> String {
    let mut out = String::new();
    canonical_into(m, &mut out);
    out
}

/// Completeness: every part's bytes survive. `exempt` skips (kind, part, _) pairs
/// an arrangement may decline (theme gallery); empty for the null theme.
#[cfg(test)]
pub(crate) fn first_dropped(
    m: &PartMap,
    out: &str,
    exempt: &[(&str, &str, &str)],
) -> Option<String> {
    for (name, part) in m.iter() {
        if exempt.iter().any(|(k, p, _)| *k == m.kind && *p == name) {
            continue;
        }
        let here = format!("{}.{name}", m.kind);
        let kept = match part {
            Part::Text(v) => out.contains(crate::render::esc(v).as_str()),
            Part::Html(v) => out.contains(v.as_str()),
            Part::Flag(true) => out.contains(&format!("data-{name}")),
            Part::Flag(false) => true,
            Part::Stream(items) => {
                if let Some(d) = items.iter().find_map(|c| first_dropped(c, out, exempt)) {
                    return Some(d);
                }
                true
            }
            Part::Map(sub) => {
                if let Some(d) = first_dropped(sub, out, exempt) {
                    return Some(d);
                }
                true
            }
        };
        if !kept {
            return Some(here);
        }
    }
    None
}

#[cfg(test)]
fn complete(m: &PartMap, out: &str) -> bool {
    first_dropped(m, out, &[]).is_none()
}

/// Fill every part of a kind with a traceable value (schema-driven; for gallery tests).
#[cfg(test)]
pub(crate) fn populate(schemas: &Schemas, kind: &str, depth: usize) -> PartMap {
    let name: &'static str = schemas
        .kind_names()
        .into_iter()
        .find(|k| *k == kind)
        .map(|k| Box::leak(k.to_string().into_boxed_str()) as &'static str)
        .expect("kind exists");
    let mut m = PartMap::new(name);
    let Some(parts) = schemas.get(kind) else {
        return m;
    };
    for (part, ty) in parts {
        match ty {
            PartType::Text => m.set(part, Part::Text(format!("text-{kind}-{part}"))),
            PartType::Url => m.set(part, Part::Text(format!("/url-{kind}-{part}/"))),
            PartType::Html => m.set(part, Part::Html(format!("<p>html-{kind}-{part}</p>"))),
            PartType::Flag => m.set(part, Part::Flag(true)),
            PartType::Stream(child) => {
                if depth > 0 {
                    m.set(
                        part,
                        Part::Stream(vec![populate(schemas, child, depth - 1)]),
                    );
                }
            }
            PartType::Map(child) => {
                if depth > 0 {
                    m.set(part, Part::Map(populate(schemas, child, depth - 1)));
                }
            }
        }
    }
    m
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
                    let _ = writeln!(out, "<a data-slot=\"{n}\" href=\"{e}\">{e}</a>");
                } else {
                    let _ = writeln!(
                        out,
                        "<span data-slot=\"{n}\">{}</span>",
                        crate::render::esc(v)
                    );
                }
            }
            Part::Html(v) => {
                let _ = writeln!(out, "<div data-slot=\"{n}\">{v}</div>");
            }
            Part::Stream(items) => {
                let _ = writeln!(out, "<div data-slot=\"{n}\">");
                for item in items {
                    canonical_into(item, out);
                }
                out.push_str("</div>\n");
            }
            Part::Map(sub) => {
                let _ = writeln!(out, "<div data-slot=\"{n}\">");
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

/// Linked pills for one list field: display name from `[records]`, URL from
/// the field's archive view when one exists.
fn pill_stream(
    cfg: &crate::config::Config,
    p: &Row,
    field: &str,
    child: &'static str,
    name_key: &'static str,
    url_key: &'static str,
) -> Part {
    let ids = match grackle_db::filter::Row::field(p, field) {
        grackle_db::Value::List(v) => v,
        _ => Vec::new(),
    };
    let v = ids
        .iter()
        .map(|id| {
            let mut m = PartMap::new_declared(child);
            m.set_declared(
                name_key,
                Part::Text(
                    cfg.record_name(field, id, &cfg.pairing_member(p))
                        .to_string(),
                ),
            );
            if let Some(url) = cfg.archive_url(field, id, &cfg.pairing_member(p)) {
                m.set_declared(url_key, Part::Text(url));
            }
            m
        })
        .collect();
    Part::Stream(v)
}

/// `(Text, Url)` child shape — archive pills (`tag`: name + url).
fn pill_keys(shape: &[(&'static str, PartType)]) -> Option<(&'static str, &'static str)> {
    if shape.len() != 2 {
        return None;
    }
    let (a, ta) = shape[0];
    let (b, tb) = shape[1];
    match (ta, tb) {
        (PartType::Text, PartType::Url) => Some((a, b)),
        (PartType::Url, PartType::Text) => Some((b, a)),
        _ => None,
    }
}

/// One neighbor: a full route row under the `neighbor` kind (same fill as a
/// listing card). Themes keep styling `[data-kind="neighbor"]`.
fn neighbor_from_row(cfg: &crate::config::Config, row: &Row) -> PartMap {
    route_face(
        cfg,
        "neighbor",
        Preview {
            row: Some(row),
            ..Default::default()
        },
    )
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
/// carries its resolved label; empties are dropped upstream. Items are
/// looked up as full rows and projected through [`neighbor_from_row`].
pub fn relation_groups(
    cfg: &crate::config::Config,
    db: &crate::model::SiteDb,
    groups: Vec<crate::relate::Group>,
) -> Vec<PartMap> {
    groups
        .into_iter()
        .filter_map(|g| {
            let items = g
                .items
                .iter()
                .filter_map(|url| db.row_by_url(url).map(|r| neighbor_from_row(cfg, r)))
                .collect();
            relation_group(&g.name, &g.label, items)
        })
        .collect()
}

/// One axis group for the axis slot (q47, §6f): a named, labelled set of
/// members, each a link with the current one flagged. Fewer than two members is
/// no switcher, so it contributes nothing (hole-algebra rule 2). This is what
/// superseded the `translations` relation — the locale switcher is one of these,
/// beside a theme switcher or any declared axis, for rows AND listing views.
pub fn axis_group(
    name: &str,
    label: &str,
    members: Vec<(String, String, bool)>,
) -> Option<PartMap> {
    if members.len() < 2 {
        return None;
    }
    // The current member's label heads the dropdown (its summary): "ledger",
    // "Français" — what you are viewing, with the rest as the menu.
    let current_label = members
        .iter()
        .find(|(_, _, c)| *c)
        .map(|(l, _, _)| l.clone())
        .unwrap_or_else(|| label.to_string());
    let items = members
        .into_iter()
        .map(|(label, url, current)| {
            let mut m = PartMap::new("axis_member");
            m.set("label", Part::Text(label));
            m.set("url", Part::Text(url));
            if current {
                m.set("current", Part::Text("true".to_string()));
            }
            m
        })
        .collect();
    let mut g = PartMap::new("axis");
    g.set("axis", Part::Text(name.to_string()));
    g.set("label", Part::Text(label.to_string()));
    g.set("current", Part::Text(current_label));
    g.set("items", Part::Stream(items));
    Some(g)
}

#[allow(clippy::too_many_arguments)]
/// One row's part map (THEME.md §2). Callers differ in what they supply —
/// crumbs, hero, relations — not in kind. List-field pills (`tags`, …) are
/// filled later by [`fill_from_fields`].
pub fn row(
    title: String,
    url: String,
    tree: bool,
    crumbs: Vec<(String, Option<String>)>,
    hero: Option<PartMap>,
    section: Vec<PartMap>,
    outline: Vec<PartMap>,
    content: &str,
    relations: Vec<PartMap>,
) -> PartMap {
    let mut m = PartMap::new("row");
    m.set("title", Part::Text(title));
    m.set("url", Part::Text(url));
    if tree {
        m.set("tree", Part::Flag(true));
    }
    m.set("crumbs", crumb_stream(crumbs));
    if let Some(h) = hero {
        m.set("hero", Part::Map(h));
    }
    if !section.is_empty() {
        m.set("section", Part::Stream(section));
    }
    if !outline.is_empty() {
        m.set("outline", Part::Stream(outline));
    }
    m.set("content", Part::Html(content.to_string()));
    if !relations.is_empty() {
        m.set("relations", Part::Stream(relations));
    }
    m
}

pub fn document(
    p: &Row,
    content: &str,
    trail: Vec<(String, Option<String>)>,
    relation_groups: Vec<PartMap>,
    outline: Vec<PartMap>,
) -> PartMap {
    row(
        p.title.clone().unwrap_or_default(),
        p.url.clone(),
        false,
        trail,
        None,
        Vec::new(),
        outline,
        content,
        relation_groups,
    )
}

/// Home → ancestors → title (inert tail).
pub fn tree_trail(
    cfg: &crate::config::Config,
    locale: &str,
    home: &str,
    title: &str,
    ancestors: &[(String, String)],
) -> Vec<(String, Option<String>)> {
    let mut v = vec![(
        cfg.i18n_string("home", locale).to_string(),
        Some(home.to_string()),
    )];
    for (u, t) in ancestors {
        v.push((t.clone(), Some(u.clone())));
    }
    v.push((title.to_string(), None));
    v
}

#[allow(clippy::too_many_arguments)]
pub fn document_tree(
    cfg: &crate::config::Config,
    locale: &str,
    home: &str,
    title: &str,
    url: &str,
    ancestors: &[(String, String)],
    section: Vec<PartMap>,
    outline: Vec<PartMap>,
    hero: Option<PartMap>,
    relation_groups: Vec<PartMap>,
    content: &str,
) -> PartMap {
    row(
        title.to_string(),
        url.to_string(),
        true,
        tree_trail(cfg, locale, home, title, ancestors),
        hero,
        section,
        outline,
        content,
        relation_groups,
    )
}

/// A row as the view sees it. Optional fields are presence-driven (q36).
/// List-field pills land via [`fill_from_fields`] after this is built.
#[derive(Default)]
pub struct Preview<'a> {
    pub row: Option<&'a Row>,
    pub content: Option<String>,
    pub truncated: bool,
    pub src: Option<String>,
    pub dims: Option<(u32, u32)>,
    pub title: Option<String>,
    pub url: Option<String>,
    pub note: Option<String>,
}

/// One presence-driven kind; faces select variants. Fill undeclared parts from
/// row fields when types line up (§5e). List fields whose child kind is
/// `(Text, Url)` become archive pills (`record_name` + `archive_url`).
pub fn fill_from_fields(
    cfg: &crate::config::Config,
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
            continue;
        }
        let Some(v) = row.fields.get(*name) else {
            continue;
        };
        let part = match (v, ty) {
            (V::Str(s), PartType::Text) => Part::Text(s.clone()),
            (V::Str(s), PartType::Url) => {
                if row.images.contains_key(*name) {
                    Part::Text(resolve_asset(s))
                } else {
                    Part::Text(s.clone())
                }
            }
            (V::Int(n), PartType::Text) => Part::Text(n.to_string()),
            (V::Bool(b), PartType::Flag) => {
                if !*b {
                    continue;
                }
                Part::Flag(true)
            }
            (V::List(items), PartType::Stream(child)) => {
                if items.is_empty() {
                    continue;
                }
                let shape = schemas.get(child).unwrap_or(&[]);
                if let Some((name_key, url_key)) = pill_keys(shape) {
                    pill_stream(cfg, row, name, child, name_key, url_key)
                } else {
                    let label = match shape {
                        [(label, PartType::Text)] => *label,
                        _ => continue,
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

/// Shared card fill for a route — under `row` (listings) or `neighbor`
/// (relation items). Only parts declared on `kind` are set.
fn route_face(cfg: &crate::config::Config, kind: &'static str, p: Preview<'_>) -> PartMap {
    let mut m = PartMap::new(kind);
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
    if let Some(d) = row.and_then(|r| r.as_date("date")) {
        let member = row
            .map(|r| cfg.pairing_member(r))
            .unwrap_or_default();
        m.set("date", Part::Text(crate::model::iso_date(d)));
        m.set(
            "date_pretty",
            Part::Text(cfg.format_date(d, "medium_date", &member)),
        );
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
        .or_else(|| row.and_then(|r| r.string("description").map(str::to_owned)))
    {
        m.set("note", Part::Text(n));
    }
    if p.truncated {
        m.set("truncated", Part::Flag(true));
    }
    if let Some(c) = &p.content {
        m.set("content", Part::Html(c.clone()));
    }
    m
}

/// A part is filled when the row answers it — one projection serves a post, a
/// book and a photograph (q36).
pub fn preview(cfg: &crate::config::Config, p: Preview) -> PartMap {
    route_face(cfg, "row", p)
}

/// Wrapper `row` for an aggregate page: furniture around already-concatenated
/// member HTML (THEME.md §3).
pub fn page_row(
    title: &str,
    url: &str,
    trail: Vec<(String, Option<String>)>,
    intro: Option<String>,
    content: String,
    pagination: Option<PartMap>,
) -> PartMap {
    let mut m = PartMap::new("row");
    m.set("title", Part::Text(title.to_string()));
    m.set("url", Part::Text(url.to_string()));
    m.set("crumbs", crumb_stream(trail));
    if let Some(i) = intro {
        m.set("intro", Part::Html(i));
    }
    m.set("content", Part::Html(content));
    if let Some(p) = pagination {
        m.set("pagination", Part::Map(p));
    }
    m
}

/// Face for a view's members: prefer `variant` when the theme ships it,
/// else `layout`. A missing variant is a partial theme (DESIGN.md), not
/// an error; a missing layout face bails.
pub fn member_face<'a>(
    fragments: &crate::assemble::binder::Fragments,
    layout: &'a str,
    variant: Option<&'a str>,
) -> anyhow::Result<&'a str> {
    if let Some(v) = variant {
        if fragments.has_face(v) {
            return Ok(v);
        }
    }
    require_face(fragments, layout)
}

/// Bail unless the theme ships `row--{face}`.
pub fn require_face<'a>(
    fragments: &crate::assemble::binder::Fragments,
    face: &'a str,
) -> anyhow::Result<&'a str> {
    if fragments.has_face(face) {
        return Ok(face);
    }
    let known = fragments.faces();
    anyhow::bail!(
        "no row face {face:?} — theme has: {}",
        if known.is_empty() {
            "(none)".to_string()
        } else {
            known.join(", ")
        }
    )
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
    fn canonical_renders_order_links_and_facts() {
        let mut m = PartMap::new("row");
        m.set("title", Part::Text("A & B".into()));
        m.set("url", Part::Text("/x/".into()));
        m.set("tree", Part::Flag(true));
        m.set("src", Part::Text("/static/x.jpg".into()));
        m.set("width", Part::Text("320".into()));
        m.set("height", Part::Text("200".into()));
        m.set("content", Part::Html("<p>hi</p>".into()));
        let out = canonical(&m);
        assert!(
            out.starts_with(r#"<section data-kind="row" data-tree>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<span data-slot="title">A &amp; B</span>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<a data-slot="url" href="/x/">/x/</a>"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<a data-slot="src" href="/static/x.jpg">"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<span data-slot="width">320</span>"#),
            "{out}"
        );
        let t = out.find("data-slot=\"title\"").unwrap();
        let c = out.find("data-slot=\"content\"").unwrap();
        assert!(t < c, "title precedes content, as the schema declares");
    }

    #[test]
    fn canonical_follows_schema_order_not_call_order() {
        let mut m = PartMap::new("row");
        m.set("content", Part::Html("<p>body</p>".into()));
        m.set("title", Part::Text("T".into()));
        let names: Vec<&str> = m.iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["title", "content"]);
        let out = canonical(&m);
        assert!(
            out.find("data-slot=\"title\"").unwrap() < out.find("data-slot=\"content\"").unwrap(),
            "{out}"
        );
    }

    /// §5e: every real row through the null theme — nothing the parts layer
    /// carries may vanish (fragments cannot put a dropped part back).
    #[test]
    fn null_theme_is_complete_over_every_real_row() {
        let cfg = crate::config::Config::load(&crate::workspace_root().join("grackle.toml"))
            .expect("grackle.toml loads");
        let db = grackle_source::load(&cfg).expect("site db loads");
        assert!(db.post_ix.len() > 300, "real corpus expected");

        for p in db.posts() {
            let trail = vec![
                ("Home".to_string(), Some("/".to_string())),
                (p.title.clone().unwrap_or_default(), None),
            ];
            let body = crate::store::read_body(&p.path).unwrap_or_default();
            let m = document(p, &body, trail, Vec::new(), Vec::new());
            let out = canonical(&m);
            assert!(complete(&m, &out), "post {} dropped a part", p.url);
        }

        for r in &db.routes {
            if r.view.is_none() || r.members.is_empty() {
                continue;
            }
            let content = r
                .members
                .iter()
                .filter_map(|k| db.rows.get(k))
                .map(|p| {
                    let mut m = preview(&cfg, Preview {
                        row: Some(p),
                        content: Some(crate::store::read_body(&p.path).unwrap_or_default()),
                        ..Default::default()
                    });
                    fill_from_fields(&cfg, &mut m, p, &Schemas::engine_only(), &|s| s.to_string())
                        .unwrap();
                    canonical(&m)
                })
                .collect::<String>();
            let m = page_row(
                r.key.as_deref().unwrap_or("listing"),
                &r.url,
                vec![("Home".to_string(), Some("/".to_string()))],
                None,
                content,
                r.page.and_then(|n| {
                    let urls: Vec<String> = (1..=66).map(|i| format!("/blog/page/{i}")).collect();
                    pagination(n, &urls)
                }),
            );
            let out = canonical(&m);
            assert!(complete(&m, &out), "listing {} dropped a part", r.url);
        }

        for pg in db.pages() {
            let title = pg.title.clone().unwrap_or_default();
            let m = document_tree(
                &cfg,
                &cfg.pairing_member(pg),
                "/",
                &title,
                &pg.url,
                &[("/code/".to_string(), "Code".to_string())],
                Vec::new(),
                Vec::new(),
                None,
                Vec::new(),
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
        assert_eq!(
            names("root"),
            ["nav", "site_title", "axes", "content", "copyright"]
        );
        assert_eq!(names("item"), ["label"], "the shape a list field fills");
        assert_eq!(
            names("row"),
            [
                "title",
                "url",
                "tree",
                "crumbs",
                "tags",
                "hero",
                "section",
                "outline",
                "intro",
                "content",
                "pagination",
                "date",
                "date_pretty",
                "src",
                "width",
                "height",
                "note",
                "truncated",
                "relations",
            ]
        );
        assert_eq!(names("axis"), ["axis", "label", "current", "items"]);
        assert_eq!(names("axis_member"), ["label", "url", "current"]);
    }

    /// A kind nothing declares is `None`, which is what makes a fragment
    /// named after a typo a load error rather than an empty render.
    #[test]
    fn an_undeclared_kind_is_none() {
        assert!(schema("row").is_some());
        assert!(schema("sumary").is_none());
    }
}

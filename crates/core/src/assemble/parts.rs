//! Part maps: typed holes a theme's fragments place.
//!
//! Part vocabulary is derived from field schemas + theme fragments, not a
//! handwritten table. Schema order is first-seen fragment order. `set` stores
//! at that position, not call order. Producers never see `Site`, URLs stay
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
    /// A specific fragment this map asks to render through, by full name,
    /// the row-variant idea one level down. Set by the engine when a
    /// positional `.slots/chrome.html` override resolves for the page;
    /// wins over the hole's `data-fragment`, falls through when absent.
    face: Option<String>,
}

impl PartMap {
    pub fn new(kind: &'static str) -> Self {
        debug_assert!(schema(kind).is_some(), "unknown part-map kind `{kind}`");
        PartMap {
            kind,
            parts: Vec::new(),
            face: None,
        }
    }

    pub fn set_face(&mut self, name: String) {
        self.face = Some(name);
    }

    pub fn face(&self) -> Option<&str> {
        self.face.as_deref()
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
        // Schema order, not call order, null theme reading order.
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
            face: None,
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

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
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

    /// Parts in canonical order, what the null theme renders.
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

/// Derived part vocabulary for a build.
static SCHEMAS: std::sync::OnceLock<Vec<(String, Vec<(String, PartType)>)>> =
    std::sync::OnceLock::new();

#[derive(Debug, Clone)]
pub struct Schemas {
    kinds: Vec<(String, Vec<(&'static str, PartType)>)>,
}

impl Schemas {
    /// Base fragments only, tests and the null theme before site fields land.
    pub fn engine_only() -> Schemas {
        Schemas { kinds: engine() }
    }

    /// Derive kinds/parts from parsed fragments, then overlay field schemas.
    pub fn derive(
        fragments: &crate::assemble::binder::Fragments,
        fields: &[(String, grackle_source::schema::FieldType)],
    ) -> Schemas {
        let mut kinds: Vec<(String, Vec<(&'static str, PartType)>)> = Vec::new();

        for (kind, uses) in fragments.slot_uses() {
            let mut parts = Vec::new();
            for u in &uses {
                let ty = infer_part_type(u);
                let name: &'static str = Box::leak(u.part.clone().into_boxed_str());
                if parts.iter().any(|(n, _)| *n == name) {
                    continue;
                }
                parts.push((name, ty));
            }
            kinds.push((kind, parts));
        }

        // Kinds that exist as fragments but declare no slots yet.
        for k in fragments.kinds() {
            if !kinds.iter().any(|(n, _)| n == k) {
                kinds.push((k.to_string(), Vec::new()));
            }
        }

        seed_furniture(&mut kinds);
        overlay_fields(&mut kinds, fields);
        Schemas { kinds }
    }

    /// Extend with a theme's `.schema.toml`: each field becomes
    /// a part on `row`. May not remove or retype an existing part.
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
                continue;
            };
            let leaked: &'static str = Box::leak(name.clone().into_boxed_str());
            let Some((_, parts)) = kinds.iter_mut().find(|(k, _)| k == "row") else {
                continue;
            };
            match parts.iter().find(|(n, _)| *n == leaked) {
                Some((_, prev)) if *prev != part_ty => anyhow::bail!(
                    "{}: part `row.{name}` is {:?}; a theme may not retype an engine part",
                    path.display(),
                    prev
                ),
                Some(_) => {}
                None => parts.push((leaked, part_ty)),
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

    fn into_static(self) -> Vec<(String, Vec<(String, PartType)>)> {
        self.kinds
            .into_iter()
            .map(|(k, parts)| {
                (
                    k,
                    parts.into_iter().map(|(n, t)| (n.to_string(), t)).collect(),
                )
            })
            .collect()
    }
}

fn field_value_as_part(val: &toml::Value) -> Option<PartType> {
    let ty = val.get("type")?.as_str()?;
    field_type_as_part(ty)
}

fn field_type_as_part(ty: &str) -> Option<PartType> {
    Some(match ty {
        "string" | "int" | "date" => PartType::Text,
        "bool" => PartType::Flag,
        "image" => PartType::Url,
        "list" | "records" => PartType::Stream("item"),
        _ => return None,
    })
}

fn field_type_enum_as_part(ty: &grackle_source::schema::FieldType) -> PartType {
    use grackle_source::schema::FieldType as F;
    match ty {
        F::Str | F::Int | F::Date => PartType::Text,
        F::Bool => PartType::Flag,
        F::Image => PartType::Url,
        F::List | F::Records { .. } => PartType::Stream("item"),
    }
}

const HTML_SLOTS: &[&str] = &["content", "intro", "nav", "copyright"];
const MAP_SLOTS: &[&str] = &["pagination", "search", "scheme", "feed", "chrome"];

fn infer_part_type(u: &crate::assemble::binder::SlotUse) -> PartType {
    if let Some(target) = u.fragment.as_deref() {
        let child = target.split_once("--").map(|(k, _)| k).unwrap_or(target);
        let child: &'static str = Box::leak(child.to_string().into_boxed_str());
        if MAP_SLOTS.contains(&u.part.as_str()) {
            return PartType::Map(child);
        }
        return PartType::Stream(child);
    }
    if u.as_url_attr {
        return PartType::Url;
    }
    if HTML_SLOTS.contains(&u.part.as_str()) {
        return PartType::Html;
    }
    PartType::Text
}

fn seed_furniture(kinds: &mut Vec<(String, Vec<(&'static str, PartType)>)>) {
    // Producer-only kinds with no base fragment.
    ensure_kind(kinds, "item", &[("label", PartType::Text)]);
    ensure_kind(kinds, "raw", &[("content", PartType::Html)]);
    // Presentation flags producers set; fragments never content-slot them.
    if let Some((_, parts)) = kinds.iter_mut().find(|(k, _)| k == "row") {
        for (name, ty) in [("tree", PartType::Flag), ("truncated", PartType::Flag)] {
            if !parts.iter().any(|(n, _)| *n == name) {
                parts.push((name, ty));
            }
        }
    }
    // Chrome parts are engine vocabulary on `root` and `chrome` whether or
    // not a given root places them: placement is the theme's option
    // (directly, or through the cluster), presence is the fact's, and the
    // fill code sets whichever level the resolved root reads.
    let widgets: &[(&'static str, PartType)] = &[
        ("axes", PartType::Stream("axis")),
        ("search", PartType::Map("search_button")),
        ("scheme", PartType::Map("scheme_button")),
        ("feed", PartType::Map("feed_link")),
    ];
    for kind in ["root", "chrome"] {
        if let Some((_, parts)) = kinds.iter_mut().find(|(k, _)| k == kind) {
            for (name, ty) in widgets {
                if !parts.iter().any(|(n, _)| n == name) {
                    parts.push((name, *ty));
                }
            }
        }
    }
}

fn ensure_kind(
    kinds: &mut Vec<(String, Vec<(&'static str, PartType)>)>,
    kind: &str,
    parts: &[(&'static str, PartType)],
) {
    if kinds.iter().any(|(k, _)| k == kind) {
        return;
    }
    kinds.push((kind.to_string(), parts.to_vec()));
}

fn overlay_fields(
    kinds: &mut Vec<(String, Vec<(&'static str, PartType)>)>,
    fields: &[(String, grackle_source::schema::FieldType)],
) {
    let Some((_, parts)) = kinds.iter_mut().find(|(k, _)| k == "row") else {
        return;
    };
    for (name, ty) in fields {
        let part_ty = field_type_enum_as_part(ty);
        let leaked: &'static str = Box::leak(name.clone().into_boxed_str());
        if parts.iter().any(|(n, _)| *n == leaked) {
            continue; // fragment-derived wins
        }
        parts.push((leaked, part_ty));
    }
}

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
        let sources = crate::base::base_sources();
        let frags =
            crate::assemble::binder::Fragments::parse(sources).expect("base fragments must parse");
        Schemas::derive(&frags, &[]).into_static()
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

/// Null theme: schema order, generic markup from part types.
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

/// Crumb trails arrive as `(label, url?)` pairs, a crumb with no url is the
/// trail's inert tail. The *derivation* lives with the caller: post and
/// listing trails are provenance walks over the view chain, which
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
    let ids = grackle_db::filter::Row::field(p, field).as_str_list();
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

/// `(Text, Url)` child shape, archive pills (`tag`: name + url).
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

/// One relation item: the full row projection; `row--neighbor` (or another
/// face) chops what the fragment places.
fn neighbor_from_row(
    cfg: &crate::config::Config,
    schemas: &Schemas,
    resolve_asset: &dyn Fn(&str) -> String,
    row: &Row,
) -> anyhow::Result<PartMap> {
    from_row(
        cfg,
        schemas,
        resolve_asset,
        row,
        Presentation::default(),
        FillOpts::default(),
    )
}

/// One relations group: a named, labelled list of neighbours. The name
/// is the theme contract, themes key CSS on `[data-relation="…"]` (renamed
/// from `data-axis` at the axis/relation split), and the `relation`
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

/// The engine's relation groups, already evaluated, as parts. Each
/// carries its resolved label; empties are dropped upstream. Items are
/// looked up as full rows and projected through [`neighbor_from_row`].
pub fn relation_groups(
    cfg: &crate::config::Config,
    db: &crate::model::SiteDb,
    schemas: &Schemas,
    resolve_asset: &dyn Fn(&str) -> String,
    groups: Vec<crate::relate::Group>,
) -> anyhow::Result<Vec<PartMap>> {
    let mut out = Vec::new();
    for g in groups {
        let mut items = Vec::new();
        for url in &g.items {
            let Some(r) = db.row_by_url(url) else {
                continue;
            };
            items.push(neighbor_from_row(cfg, schemas, resolve_asset, r)?);
        }
        if let Some(group) = relation_group(&g.name, &g.label, items) {
            out.push(group);
        }
    }
    Ok(out)
}

/// One axis group for the axis slot: a named, labelled set of
/// members, each a link with the current one flagged. Fewer than two members is
/// no switcher, so it contributes nothing (hole-algebra rule 2). This is what
/// superseded the `translations` relation, the locale switcher is one of these,
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
    // "Français", what you are viewing, with the rest as the menu.
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
/// One row's part map. Callers differ in what they supply,
/// crumbs, relations, not in kind. List-field pills (`tags`, …) and
/// computed Str fields (`hero`) are filled later by [`fill_from_fields`].
pub fn row(
    title: String,
    url: String,
    tree: bool,
    crumbs: Vec<(String, Option<String>)>,
    section: Vec<PartMap>,
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
    if !section.is_empty() {
        m.set("section", Part::Stream(section));
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
) -> PartMap {
    row(
        p.title.clone().unwrap_or_default(),
        p.url.clone(),
        false,
        trail,
        Vec::new(),
        content,
        relation_groups,
    )
}

/// Home -> ancestors -> title (inert tail).
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
    relation_groups: Vec<PartMap>,
    content: &str,
) -> PartMap {
    row(
        title.to_string(),
        url.to_string(),
        true,
        tree_trail(cfg, locale, home, title, ancestors),
        section,
        content,
        relation_groups,
    )
}

/// Presentation facts that are not plain row fields: thumb `hero`/dims,
/// truncated body, and rare overrides (object stem title).
#[derive(Default)]
pub struct Presentation {
    pub content: Option<String>,
    pub truncated: bool,
    /// Picture URL for faces that bind `data-slot-src="hero"` (objects).
    pub hero: Option<String>,
    pub dims: Option<(u32, u32)>,
    pub title: Option<String>,
    pub url: Option<String>,
}

/// Options for [`fill_from_fields`]: which view's computed fields to eval,
/// body blocks for `content`, and thumbs for Url dims.
#[derive(Default)]
pub struct FillOpts<'a> {
    pub view: Option<&'a str>,
    pub blocks: Option<&'a [String]>,
    pub thumbs: Option<&'a crate::thumbs::Renditions>,
}

fn apply_presentation(m: &mut PartMap, p: Presentation) {
    if let Some(t) = p.title {
        m.set("title", Part::Text(t));
    }
    if let Some(u) = p.url {
        m.set("url", Part::Text(u));
    }
    if let Some(s) = p.hero {
        m.set("hero", Part::Text(s));
    }
    if let Some((w, h)) = p.dims {
        m.set("width", Part::Text(w.to_string()));
        m.set("height", Part::Text(h.to_string()));
    }
    if p.truncated {
        m.set("truncated", Part::Flag(true));
    }
    if let Some(c) = p.content {
        m.set("content", Part::Html(c));
    }
}

/// A row shaped only by presentation (picture cards with no backing content row).
pub fn present(p: Presentation) -> PartMap {
    let mut m = PartMap::new("row");
    apply_presentation(&mut m, p);
    m
}

fn record_part_map(
    child: &'static str,
    shape: &[(&'static str, PartType)],
    entries: &[(grackle_db::Value, grackle_db::Value)],
) -> PartMap {
    use grackle_db::Value as V;
    let mut item = PartMap::new_declared(child);
    for (part, ty) in shape {
        let val = entries.iter().find_map(|(k, v)| match k {
            V::Str(s) if s == *part => Some(v),
            _ => None,
        });
        match (val, ty) {
            (Some(V::Str(s)), PartType::Text) => {
                item.set_declared(part, Part::Text(s.clone()));
            }
            (Some(V::Int(n)), PartType::Text) => {
                item.set_declared(part, Part::Text(n.to_string()));
            }
            (Some(V::Bool(true)), PartType::Flag) => {
                item.set_declared(part, Part::Flag(true));
            }
            _ => {}
        }
    }
    item
}

/// Full row projection for listings and relations. The face chops what
/// it places; this fills everything the row can answer.
pub fn from_row(
    cfg: &crate::config::Config,
    schemas: &Schemas,
    resolve_asset: &dyn Fn(&str) -> String,
    row: &Row,
    p: Presentation,
    opts: FillOpts<'_>,
) -> anyhow::Result<PartMap> {
    let mut m = present(p);
    fill_from_fields(cfg, &mut m, row, schemas, resolve_asset, opts)?;
    Ok(m)
}

/// One presence-driven kind; faces select variants. Fill undeclared parts from
/// the row when types line up, schema fields plus row columns (`title`,
/// `url`, …), then computed Str fields (`hero`). `date_pretty` is the formatted
/// twin of a date-typed `date` field. List fields whose child kind is
/// `(Text, Url)` become archive pills.
pub fn fill_from_fields(
    cfg: &crate::config::Config,
    m: &mut PartMap,
    row: &Row,
    schemas: &Schemas,
    resolve_asset: &dyn Fn(&str) -> String,
    opts: FillOpts<'_>,
) -> anyhow::Result<()> {
    use grackle_db::filter::Row as FilterRow;
    use grackle_db::Value as V;
    let Some(decl) = schemas.get(m.kind) else {
        return Ok(());
    };
    for (name, ty) in decl {
        if m.get(name).is_some() {
            continue;
        }
        if *name == "date_pretty" {
            let Some(d) = row.as_date("date") else {
                continue;
            };
            let member = cfg.pairing_member(row);
            m.set_declared(name, Part::Text(cfg.format_date(d, "medium_date", &member)));
            continue;
        }
        let v = FilterRow::field(row, name);
        let part = match (&v, ty) {
            (V::Null, _) => continue,
            (V::Str(s), PartType::Text) => Part::Text(s.clone()),
            (V::Str(s), PartType::Url) => {
                set_url_with_dims(m, name, s, resolve_asset, opts.thumbs);
                continue;
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
                } else if items.iter().all(|v| matches!(v, V::Map(_))) {
                    Part::Stream(
                        items
                            .iter()
                            .filter_map(|v| match v {
                                V::Map(entries) => Some(record_part_map(child, shape, entries)),
                                _ => None,
                            })
                            .collect(),
                    )
                } else {
                    let label = match shape {
                        [(label, PartType::Text)] => *label,
                        _ => continue,
                    };
                    Part::Stream(
                        items
                            .iter()
                            .filter_map(|v| match v {
                                V::Str(s) => Some(s),
                                _ => None,
                            })
                            .map(|s| {
                                let mut item = PartMap::new_declared(child);
                                item.set_declared(label, Part::Text(s.clone()));
                                item
                            })
                            .collect(),
                    )
                }
            }
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
    fill_computed_str_fields(cfg, m, row, decl, resolve_asset, &opts);
    fill_computed_content_fields(cfg, m, row, decl, &opts);
    fill_computed_outline_fields(cfg, m, row, decl, &opts);
    Ok(())
}

fn set_url_with_dims(
    m: &mut PartMap,
    name: &'static str,
    path: &str,
    resolve_asset: &dyn Fn(&str) -> String,
    thumbs: Option<&crate::thumbs::Renditions>,
) {
    m.set_declared(name, Part::Text(resolve_asset(path)));
    if m.get("width").is_some() {
        return;
    }
    let Some(thumbs) = thumbs else {
        return;
    };
    let Some(t) = crate::thumbs::default_of(thumbs, path) else {
        return;
    };
    if let Some((w, h)) = t.dims {
        m.set("width", Part::Text(w.to_string()));
        m.set("height", Part::Text(h.to_string()));
    }
}

/// Eval computed Str fields (`fields.hero`, …) into empty Url/Text parts.
fn fill_computed_str_fields(
    cfg: &crate::config::Config,
    m: &mut PartMap,
    row: &Row,
    decl: &[(&'static str, PartType)],
    resolve_asset: &dyn Fn(&str) -> String,
    opts: &FillOpts<'_>,
) {
    let content = grackle_db::Content::new(opts.blocks.map(|b| b.to_vec()).unwrap_or_default());
    let bind = crate::passes::preview::FieldBind { row, content };
    for (name, expr) in computed_field_exprs(cfg, opts.view, grackle_db::Type::Str) {
        if m.get(name).is_some() {
            continue;
        }
        let Some(&(static_name, ty)) = decl.iter().find(|(n, _)| *n == name) else {
            continue;
        };
        if !matches!(ty, PartType::Url | PartType::Text) {
            continue;
        }
        let grackle_db::Value::Str(s) = expr.eval(&bind) else {
            continue;
        };
        if s.is_empty() {
            continue;
        }
        match ty {
            PartType::Url => set_url_with_dims(m, static_name, &s, resolve_asset, opts.thumbs),
            PartType::Text => {
                m.set_declared(static_name, Part::Text(s));
            }
            _ => {}
        }
    }
}

/// Eval a computed Outline field (`fields.outline`) into an empty
/// `outline_entry` stream; heading anchors resolve against the row's URL.
fn fill_computed_outline_fields(
    cfg: &crate::config::Config,
    m: &mut PartMap,
    row: &Row,
    decl: &[(&'static str, PartType)],
    opts: &FillOpts<'_>,
) {
    let content = grackle_db::Content::new(opts.blocks.map(|b| b.to_vec()).unwrap_or_default());
    let bind = crate::passes::preview::FieldBind { row, content };
    for (name, expr) in computed_field_exprs(cfg, opts.view, grackle_db::Type::Outline) {
        if m.get(name).is_some() {
            continue;
        }
        let Some(&(static_name, ty)) = decl.iter().find(|(n, _)| *n == name) else {
            continue;
        };
        if !matches!(ty, PartType::Stream(_)) {
            continue;
        }
        let grackle_db::Value::Outline(nodes) = expr.eval(&bind) else {
            continue;
        };
        let tree: Vec<crate::outline::Node> = nodes.into_iter().map(db_outline_to_node).collect();
        let parts = crate::outline::to_parts(&tree, &row.url);
        if parts.is_empty() {
            continue;
        }
        m.set_declared(static_name, Part::Stream(parts));
    }
}

fn db_outline_to_node(n: grackle_db::OutlineNode) -> crate::outline::Node {
    crate::outline::Node {
        label: n.label,
        url: n.url,
        order: None,
        children: n.children.into_iter().map(db_outline_to_node).collect(),
    }
}

/// Eval computed Content fields (`fields.lede`, …) into empty Html parts.
fn fill_computed_content_fields(
    cfg: &crate::config::Config,
    m: &mut PartMap,
    row: &Row,
    decl: &[(&'static str, PartType)],
    opts: &FillOpts<'_>,
) {
    let content = grackle_db::Content::new(opts.blocks.map(|b| b.to_vec()).unwrap_or_default());
    let bind = crate::passes::preview::FieldBind { row, content };
    for (name, expr) in computed_field_exprs(cfg, opts.view, grackle_db::Type::Content) {
        if m.get(name).is_some() {
            continue;
        }
        let Some(&(static_name, ty)) = decl.iter().find(|(n, _)| *n == name) else {
            continue;
        };
        if !matches!(ty, PartType::Html) {
            continue;
        }
        let grackle_db::Value::Content(c) = expr.eval(&bind) else {
            continue;
        };
        let html = c.html_string();
        if html.is_empty() {
            continue;
        }
        m.set_declared(static_name, Part::Html(html));
        if c.truncated && m.get("truncated").is_none() {
            m.set("truncated", Part::Flag(true));
        }
    }
}

/// Computed field expressions of one return type: a view's `fields_for`
/// (which already folds in `[schema.fields]`), or the bag alone for a
/// document render. The type is inferred from each expression, so a
/// field routes to the filler that wants its shape regardless of its name.
fn computed_field_exprs<'a>(
    cfg: &'a crate::config::Config,
    view: Option<&str>,
    ty: grackle_db::Type,
) -> Vec<(&'a str, grackle_db::FieldExpr)> {
    let named: Vec<(&str, &crate::config::Field)> = match view {
        Some(v) => cfg.fields_for(v).into_iter().collect(),
        None => cfg
            .schema_fields()
            .iter()
            .map(|(n, f)| (n.as_str(), f))
            .collect(),
    };
    let schema = cfg.field_expr_schema();
    named
        .into_iter()
        .filter_map(|(n, f)| Some((n, f.as_expr()?)))
        .filter_map(|(n, src)| {
            let expr = grackle_db::FieldExpr::infer(src, &schema)
                .expect("computed field validated at load");
            (expr.returns() == ty).then_some((n, expr))
        })
        .collect()
}

/// Wrapper `row` for an aggregate page: furniture around already-concatenated
/// member HTML.
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
/// else `layout`. A missing variant is a partial theme (design.md), not
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
        crate::util::join_or_none(&known)
    )
}

/// one genuine component, as data: prev/next (absent at the ends) and
/// the page range (a page with no `url` is the current one). `None` when there
/// is a single page.
///
/// `urls[i]` is page i+1's link target, rendered by build from the owning
/// view's route templates, this producer knows nothing about blogs.
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

/// The row's content *is* `main`, the null theme wraps it, real themes
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
        m.set("hero", Part::Text("/static/x.jpg".into()));
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
            out.contains(r#"<a data-slot="hero" href="/static/x.jpg">"#),
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

    /// Every real row through the null theme, nothing the parts layer
    /// carries may vanish (fragments cannot put a dropped part back).
    #[test]
    fn null_theme_is_complete_over_every_real_row() {
        let cfg = crate::config::Config::load(&crate::workspace_root().join("grackle.toml"))
            .expect("grackle.toml loads");
        let db = grackle_source::load(&cfg).expect("site db loads");
        assert!(db.rows.len() > 300, "real corpus expected");

        for p in db.rows.iter().filter(|p| cfg.body_held(p)) {
            let trail = vec![
                ("Home".to_string(), Some("/".to_string())),
                (p.title.clone().unwrap_or_default(), None),
            ];
            let body = crate::store::read_body(&p.path).unwrap_or_default();
            let m = document(p, &body, trail, Vec::new());
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
                    let m = from_row(
                        &cfg,
                        &Schemas::engine_only(),
                        &|s| s.to_string(),
                        p,
                        Presentation {
                            content: Some(crate::store::read_body(&p.path).unwrap_or_default()),
                            ..Default::default()
                        },
                        FillOpts::default(),
                    )
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

        for pg in db.rows.iter().filter(|p| {
            cfg.collections
                .get(&p.collection)
                .is_some_and(|c| c.is_tree())
                && p.rendered
        }) {
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

    fn base_schemas() -> Schemas {
        Schemas::engine_only()
    }

    #[test]
    fn every_child_kind_resolves() {
        let schemas = base_schemas();
        for kind in schemas.kind_names() {
            for (part, ty) in schemas.get(kind).unwrap() {
                let child = match ty {
                    PartType::Stream(k) | PartType::Map(k) => *k,
                    _ => continue,
                };
                assert!(
                    schemas.get(child).is_some(),
                    "{kind}.{part} names kind {child:?}, which is not declared"
                );
            }
        }
    }

    #[test]
    fn every_base_fragment_slot_is_declared() {
        let sources = crate::base::base_sources();
        let frags = crate::assemble::binder::Fragments::parse(sources).unwrap();
        let schemas = Schemas::derive(&frags, &[]);
        for (kind, uses) in frags.slot_uses() {
            let parts = schemas.get(&kind).unwrap_or(&[]);
            for u in uses {
                assert!(
                    parts.iter().any(|(n, _)| *n == u.part),
                    "slot `{}.{}` missing from derived schema",
                    kind,
                    u.part
                );
            }
        }
    }

    #[test]
    fn field_schema_overlays_row() {
        use grackle_source::schema::FieldType;
        let sources = crate::base::base_sources();
        let frags = crate::assemble::binder::Fragments::parse(sources).unwrap();
        let fields = vec![
            ("date".into(), FieldType::Date),
            ("description".into(), FieldType::Str),
            ("score".into(), FieldType::Int),
        ];
        let schemas = Schemas::derive(&frags, &fields);
        let row = schemas.get("row").unwrap();
        assert!(row
            .iter()
            .any(|(n, t)| *n == "date" && *t == PartType::Text));
        // description is a card-face slot; score arrives only via overlay
        assert!(row.iter().any(|(n, _)| *n == "description"));
        assert!(row
            .iter()
            .any(|(n, t)| *n == "score" && *t == PartType::Text));
    }

    #[test]
    fn relation_items_are_rows() {
        let schemas = base_schemas();
        let relation = schemas.get("relation").expect("relation kind");
        let items = relation
            .iter()
            .find(|(n, _)| *n == "items")
            .expect("relation.items");
        assert_eq!(items.1, PartType::Stream("row"));
        assert!(schemas.get("neighbor").is_none());
    }

    #[test]
    fn fill_from_fields_reads_columns_and_date_pretty() {
        use grackle_db::Value as V;
        let cfg = crate::config::Config::load(&crate::workspace_root().join("grackle.toml"))
            .expect("grackle.toml loads");
        let mut row = Row {
            title: Some("Hello".into()),
            url: "/hello/".into(),
            rel: "posts/hello.md".into(),
            ..Default::default()
        };
        row.fields
            .insert("date".into(), V::Str("2026-07-30".into()));
        row.fields
            .insert("description".into(), V::Str("a blurb".into()));

        let mut m = PartMap::new("row");
        fill_from_fields(
            &cfg,
            &mut m,
            &row,
            &Schemas::engine_only(),
            &|s| s.to_string(),
            FillOpts::default(),
        )
        .unwrap();
        assert_eq!(m.text("title"), Some("Hello"));
        assert_eq!(m.text("url"), Some("/hello/"));
        assert_eq!(m.text("date"), Some("2026-07-30"));
        assert_eq!(m.text("description"), Some("a blurb"));
        assert!(m.text("date_pretty").is_some_and(|s| !s.is_empty()));

        let m = from_row(
            &cfg,
            &Schemas::engine_only(),
            &|s| s.to_string(),
            &row,
            Presentation {
                title: Some("Override".into()),
                ..Default::default()
            },
            FillOpts::default(),
        )
        .unwrap();
        assert_eq!(m.text("title"), Some("Override"));
    }

    #[test]
    fn an_undeclared_kind_is_none() {
        assert!(schema("row").is_some());
        assert!(schema("sumary").is_none());
    }
}

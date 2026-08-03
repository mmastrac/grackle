//! Authored config surface and merge laws.

use crate::config::effective::{collection_seg, index_seg, Prov, Trace};
use crate::markers::MarkerDef;
use crate::shape::{annotated, field, Law, Shape, Shaped};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// `"default"` merges base under this file; `"none"` declares everything (§4d).
    #[serde(default = "default_extends")]
    pub extends: String,
    #[serde(default = "default_root")]
    pub root: PathBuf,
    /// Honour .gitignore when walking (default true); see store::walker.
    #[serde(default = "default_true")]
    pub gitignore: bool,
    /// Metadata providers overlaid on every row, each under its own accessor
    /// namespace (`["git"]` -> `.git.*`). Empty means no overlay, no cost.
    #[serde(default)]
    pub metadata: Vec<String>,
    pub site: Site,
    /// Keyed by table name; built at load from `declared_collections`.
    #[serde(skip)]
    pub collections: BTreeMap<String, Collection>,
    /// Array of collections: table name from source dir (`_posts` -> `posts`);
    /// `name =` overrides; `.` falls back to `entries` (q51).
    #[serde(default, rename = "collections")]
    pub(crate) declared_collections: Vec<Collection>,
    /// Sets + routes folded; one namespace with collections via `from` (§5c).
    #[serde(skip)]
    pub views: BTreeMap<String, View>,
    /// Queries with no `path`.
    #[serde(default)]
    pub(crate) sets: BTreeMap<String, View>,
    /// Queries that land at a URL.
    #[serde(default)]
    pub(crate) routes: BTreeMap<String, View>,
    /// Alternative forms of a row; sole exception to one-row-one-route (q53, §4).
    #[serde(default)]
    pub axes: BTreeMap<String, Axis>,
    /// Marker filename -> defaults for that directory and below (DESIGN.md §4b).
    #[serde(default)]
    pub markers: BTreeMap<String, MarkerDef>,
    /// Head tags and root-element attributes (§4e).
    #[serde(default)]
    pub html: HtmlCfg,
    /// Extension -> media type for `rel="alternate"` etc. (q53).
    #[serde(default)]
    pub media_types: BTreeMap<String, String>,
    /// Typed fields + embeddings/search consumers (§5b).
    #[serde(default)]
    pub schema: SchemaBag,
    /// `{% name %}...{% endname %}` expands with `{body}` (§5d).
    #[serde(default)]
    pub widgets: BTreeMap<String, WidgetDef>,
    /// External command shells: stdin JSON, stdout at the route (§5g).
    #[serde(default)]
    pub shells: BTreeMap<String, ShellDef>,
    /// Display strings for the pairing axis; default axis `"locale"` (§6f).
    #[serde(default)]
    pub i18n: I18nCfg,
    /// Unrouted-asset embed policy (IO.md §4a, I11). Base ships on.
    #[serde(default)]
    pub embeds: EmbedsCfg,
    /// `[records.<field>.<id>]`: slug, locale names, intro for grouped values (§6f).
    #[serde(default)]
    pub records: BTreeMap<String, BTreeMap<String, RecordCfg>>,
    /// Projection of the same database: which rows, URL space, style marker (§4a).
    #[serde(default)]
    pub profiles: BTreeMap<String, ProfileCfg>,
    /// In force after `apply_profile`; `None` = as written.
    #[serde(skip)]
    pub profile: Option<String>,
    /// Links by source path / `view:`; `strict` errors on raw internal URLs (§6a).
    #[serde(default)]
    pub links: LinksCfg,
    #[serde(skip)]
    pub dir: PathBuf,
    /// Excluded by identity so the config file never publishes as content.
    #[serde(skip)]
    pub config_file: PathBuf,
    /// `[profiles.NAME.force]` lifted for the loader (MERGE.md E1).
    #[serde(skip)]
    pub forced: BTreeMap<String, toml::Value>,
}

/// Fenced config overlay plus rung-0 `force` (MERGE.md E2). Body stays TOML
/// so it merges through [`Config::shape`] like any other table.
#[derive(Debug, Deserialize, Default)]
#[serde(transparent)]
pub struct ProfileCfg {
    body: toml::Table,
}

/// Rung-0 veto key (MERGE.md E1); spelling shared by fence, split, and errors.
pub(crate) const FORCE: &str = "force";

impl ProfileCfg {
    pub(crate) fn body(&self) -> &toml::Table {
        &self.body
    }
}

/// Split overlay from `force`. Called twice (every profile at load; selected
/// one on raw TOML); must not drift.
pub(crate) fn split_profile(pname: &str, body: &toml::Table) -> Result<(toml::Table, toml::Table)> {
    let mut overlay = body.clone();
    let force = match overlay.remove(FORCE) {
        None => toml::Table::new(),
        Some(toml::Value::Table(t)) => t,
        Some(other) => anyhow::bail!(
            "profile {pname}: [profiles.{pname}.{FORCE}] is a table of \
             schema-declared field names to values (rung 0, §2), not {}",
            other.type_str()
        ),
    };
    Ok((overlay, force))
}

pub(crate) fn default_extends() -> String {
    "default".to_string()
}

/// Built-in base config (§4d); compiled in so a site cannot forget it.
pub(crate) const BASE: &str = include_str!("../../assets/base.toml");

/// Exhaustive field pin: a new [`Config`] field fails here until [`Config::shape`]
/// names it (merge runs on `toml::Value` before a typed Config exists).
#[allow(dead_code)]
fn every_config_key_has_a_law(c: Config) {
    let Config {
        extends: _,
        root: _,
        gitignore: _,
        metadata: _,
        site: _,
        declared_collections: _,
        sets: _,
        routes: _,
        axes: _,
        markers: _,
        html: _,
        media_types: _,
        schema: _,
        widgets: _,
        shells: _,
        i18n: _,
        embeds: _,
        records: _,
        profiles: _,
        links: _,
        // `#[serde(skip)]` / derived at load: no TOML key.
        collections: _,
        views: _,
        profile: _,
        dir: _,
        config_file: _,
        forced: _,
    } = c;
}

/// Merge surface: `merge_base` reads each key's law via `law_of` (§3 table A).
impl Shaped for Config {
    fn shape() -> Shape {
        Shape::Struct(vec![
            field("extends", |c: &Config| &c.extends),
            field("root", |c: &Config| &c.root),
            field("gitignore", |c: &Config| &c.gitignore),
            field("metadata", |c: &Config| &c.metadata),
            field("site", |c: &Config| &c.site),
            // TOML name (serde rename); Law::Collections: identity is physical (§1).
            annotated(
                "collections",
                |c: &Config| &c.declared_collections,
                Law::Collections,
            ),
            field("sets", |c: &Config| &c.sets),
            field("routes", |c: &Config| &c.routes),
            field("axes", |c: &Config| &c.axes),
            field("markers", |c: &Config| &c.markers),
            field("html", |c: &Config| &c.html),
            field("media_types", |c: &Config| &c.media_types),
            field("schema", |c: &Config| &c.schema),
            field("widgets", |c: &Config| &c.widgets),
            field("shells", |c: &Config| &c.shells),
            field("i18n", |c: &Config| &c.i18n),
            field("embeds", |c: &Config| &c.embeds),
            field("records", |c: &Config| &c.records),
            field("profiles", |c: &Config| &c.profiles),
            field("links", |c: &Config| &c.links),
        ])
    }
}

/// Keys a profile may write (§4a, MERGE.md E2). Exhaustive with [`NOT_PROJECTABLE`].
pub(crate) const PROJECTABLE: &[&str] = &[
    "site", "html", "sets", "routes", "i18n", "records", "widgets", "shells", "axes",
    "media_types",
];

/// Keys a profile may not write: what loads. Database identical under every
/// profile (§4a). `profiles` itself: overlay is one layer, not a ladder.
pub(crate) const NOT_PROJECTABLE: &[&str] = &[
    "collections",
    // Addresses are load facts; toggling embeds would change the database.
    "embeds",
    "schema",
    "markers",
    "root",
    "gitignore",
    "metadata",
    "extends",
    "links",
    "profiles",
];

/// Fence one top-level profile key. `force` passes: rung 0, lifted by [`split_profile`].
pub(crate) fn fence(pname: &str, key: &str) -> Result<()> {
    if key == FORCE || PROJECTABLE.contains(&key) {
        return Ok(());
    }
    let projectable = PROJECTABLE.join(", ");
    // Migration spellings still seen in shipped configs / DESIGN.md.
    if key == "noindex" {
        anyhow::bail!(
            "profile {pname}: `noindex` is no longer a profile key — it \
             overwrote [html.head.meta] robots with a constant, which \
             silently replaced a site's own expression. Force the FIELD \
             instead, and the expression evaluates it:\n  \
             [profiles.{pname}.{FORCE}]\n  noindex = true"
        );
    }
    if key == "url" {
        anyhow::bail!(
            "profile {pname}: `url` is no longer a profile key of its own — a \
             profile is a config OVERLAY now (§4a), so it writes the site's own \
             key at the site's own path:\n  \
             [profiles.{pname}.site]\n  url = \"https://drafts.example.com\""
        );
    }
    if key == "profiles" {
        anyhow::bail!(
            "profile {pname}: [profiles.{pname}.profiles] — a profile never \
             contains profiles. A projection is one overlay over the config, \
             not a ladder of them (§4a). A profile may write: {projectable}."
        );
    }
    if NOT_PROJECTABLE.contains(&key) {
        anyhow::bail!(
            "profile {pname}: [profiles.{pname}.{key}] — a profile never changes \
             what loads; the database is identical under every profile (§4a). \
             {key:?} decides what the engine reads and how it is typed, so it is \
             site config: every projection of this site sees the same rows, and \
             a profile chooses among them.\n  \
             A profile may write: {projectable}."
        );
    }
    anyhow::bail!(
        "profile {pname}: [profiles.{pname}.{key}] names no config key — a \
         profile's body is a partial config (§4a), so its top-level keys are \
         the config's own.\n  A profile may write: {projectable}."
    )
}

impl Shaped for Collection {
    fn shape() -> Shape {
        Shape::Struct(vec![
            field("name", |c: &Collection| &c.name),
            field("source", |c: &Collection| &c.source),
            field("file", |c: &Collection| &c.file),
            field("exclude", |c: &Collection| &c.exclude),
            field("include", |c: &Collection| &c.include),
            // Site rules first: Law 1 as list order (§1).
            annotated("rules", |c: &Collection| &c.rules, Law::Prepend),
            field("trail", |c: &Collection| &c.trail),
            field("archives", |c: &Collection| &c.archives),
            field("relations", |c: &Collection| &c.relations),
            field("schema", |c: &Collection| &c.schema),
        ])
    }
}

impl Shaped for Site {
    fn shape() -> Shape {
        Shape::Struct(vec![
            field("url", |s: &Site| &s.url),
            field("baseurl", |s: &Site| &s.baseurl),
            field("title", |s: &Site| &s.title),
            field("author", |s: &Site| &s.author),
            field("email", |s: &Site| &s.email),
            field("theme", |s: &Site| &s.theme),
        ])
    }
}

impl Shaped for HtmlCfg {
    fn shape() -> Shape {
        Shape::Struct(vec![
            field("head", |h: &HtmlCfg| &h.head),
            field("html", |h: &HtmlCfg| &h.html),
            field("body", |h: &HtmlCfg| &h.body),
        ])
    }
}

impl Shaped for HeadCfg {
    fn shape() -> Shape {
        Shape::Struct(vec![
            field("meta", |h: &HeadCfg| &h.meta),
            field("property", |h: &HeadCfg| &h.property),
            field("link", |h: &HeadCfg| &h.link),
            field("jsonld", |h: &HeadCfg| &h.jsonld),
        ])
    }
}

impl Shaped for AttrCfg {
    fn shape() -> Shape {
        Shape::Struct(vec![field("attribute", |a: &AttrCfg| &a.attribute)])
    }
}

impl Shaped for I18nCfg {
    fn shape() -> Shape {
        Shape::Struct(vec![
            field("axis", |i: &I18nCfg| &i.axis),
            field("names", |i: &I18nCfg| &i.names),
            field("strings", |i: &I18nCfg| &i.strings),
            field("tables", |i: &I18nCfg| &i.tables),
        ])
    }
}

impl Shaped for EmbedsCfg {
    fn shape() -> Shape {
        Shape::Struct(vec![
            field("enabled", |e: &EmbedsCfg| &e.enabled),
            // TOML name `match`, not Rust's `patterns`.
            field("match", |e: &EmbedsCfg| &e.patterns),
        ])
    }
}

impl Shaped for LinksCfg {
    fn shape() -> Shape {
        Shape::Struct(vec![field("policy", |l: &LinksCfg| &l.policy)])
    }
}

/// String-spelled enums: atoms; nothing inside for a merge to take apart.
macro_rules! enums_are_atoms {
    ($($t:ty),* $(,)?) => { $(impl Shaped for $t {
        fn shape() -> Shape { Shape::Atom }
    })* };
}

enums_are_atoms![LinkPolicy];

/// Table-spelled atom (§3 table D): compose per locale would invent a string
/// nobody wrote. [`Shape::TableAtom`] trips if a descent tries to split it.
impl Shaped for LocalizedStr {
    fn shape() -> Shape {
        Shape::TableAtom
    }
}

/// Structs under a user-chosen name; Law 2 stops here ([`Shape::definition`]).
macro_rules! definitions {
    ($($t:ty),* $(,)?) => { $(impl Shaped for $t {
        fn shape() -> Shape { Shape::definition() }
    })* };
}

definitions![
    View,
    Axis,
    ShellDef,
    WidgetDef,
    ProfileCfg,
    RecordCfg,
    RelationCfg,
    MarkerDef,
];

/// Law for `key` from the owning field's type / annotation. Unknown keys merge
/// as atoms until `deny_unknown_fields`.
pub(crate) fn law_of(shape: &Shape, key: &str) -> Law {
    shape
        .fields()
        .iter()
        .find(|(k, _)| *k == key)
        .map_or(Law::Atom, |(_, s)| s.law())
}

fn merge_by(
    law: Law,
    base: toml::Value,
    site: toml::Value,
    path: &mut Vec<String>,
    t: &mut Trace,
) -> toml::Value {
    match law {
        Law::Atom => {
            t.record(path, t.near_over());
            site
        }
        Law::Descend(n) => merge_to_depth(base, site, n, path, t),
        Law::Collections => merge_collection_list(base, site, path, t),
        Law::Prepend => prepend(base, site, path, t),
    }
}

/// Site's array in front of base's; non-arrays leave the site whole.
fn prepend(
    base: toml::Value,
    site: toml::Value,
    path: &mut Vec<String>,
    t: &mut Trace,
) -> toml::Value {
    let (Some(b), Some(s)) = (base.as_array(), site.as_array()) else {
        t.record(path, t.near_over());
        return site;
    };
    let mut out = s.clone();
    out.extend(b.iter().cloned());
    // Provenance is list index: site front, base tail.
    if t.on() {
        for i in 0..out.len() {
            path.push(index_seg(i));
            match (i < s.len(), t.far()) {
                (true, _) => t.record(path, t.near()),
                (false, Some(far)) => t.record(path, far),
                (false, None) => {}
            }
            path.pop();
        }
    }
    toml::Value::Array(out)
}

/// Serde defaults for keys neither writer set; same fns as `#[serde(default)]`.
pub(crate) fn engine_defaults() -> Vec<(&'static str, toml::Value)> {
    vec![
        ("extends", default_extends().into()),
        ("root", default_root().display().to_string().into()),
        ("gitignore", default_true().into()),
    ]
}

/// Merge base under site (§4d); laws come from [`Config::shape`].
pub(crate) fn merge_base(site: toml::Value) -> Result<toml::Value> {
    merge_base_traced(site, &mut Trace::off())
}

/// Same merge with a recorder; `--effective` entry point.
pub(crate) fn merge_base_traced(site: toml::Value, t: &mut Trace) -> Result<toml::Value> {
    let base: toml::Value =
        toml::from_str(BASE).context("parsing the built-in base config (this is an engine bug)")?;
    Ok(merge_table(
        base,
        site,
        &Config::shape(),
        &mut Vec::new(),
        t,
    ))
}

/// Project through one profile (§4a, MERGE.md E2): overlay minus `force`,
/// profile as nearer writer. Returns projected table, force block, and view
/// names the overlay wrote (MERGE.md C6a).
pub(crate) fn project(
    merged: toml::Value,
    name: &str,
    t: &mut Trace,
) -> Result<(toml::Value, toml::Table, Vec<String>)> {
    let declared = merged.get("profiles").and_then(|p| p.as_table());
    let body = match declared.and_then(|p| p.get(name)) {
        Some(b) => b.clone(),
        // `dev` is implicit (§4a): undeclared changes nothing.
        None if name == "dev" => return Ok((merged, toml::Table::new(), Vec::new())),
        None => {
            let mut known: Vec<&str> = declared
                .map(|p| p.keys().map(String::as_str).collect())
                .unwrap_or_default();
            known.push("dev");
            known.sort_unstable();
            anyhow::bail!("unknown profile {name:?} — declared: {}", known.join(", "));
        }
    };
    let Some(body) = body.as_table() else {
        anyhow::bail!(
            "profile {name}: [profiles.{name}] is a partial config — a table of \
             the config's own keys (§4a) — not {}",
            body.type_str()
        );
    };
    for key in body.keys() {
        fence(name, key)?;
    }
    let (overlay, force) = split_profile(name, body)?;
    // Before `merge_queries` folds sets/routes into one namespace.
    let patched: Vec<String> = ["sets", "routes"]
        .iter()
        .filter_map(|k| overlay.get(*k)?.as_table())
        .flat_map(|tbl| tbl.keys().cloned())
        .collect();
    t.layer(Prov::Profile);
    let projected = merge_table(
        merged,
        toml::Value::Table(overlay),
        &Config::shape(),
        &mut Vec::new(),
        t,
    );
    Ok((projected, force, patched))
}

/// Record `prov` for every atom under `path`, same depth the merge would use.
fn note_key(t: &mut Trace, path: &mut Vec<String>, law: Law, v: &toml::Value, prov: Prov) {
    if !t.on() {
        return;
    }
    match law {
        Law::Atom => t.record(path, prov),
        Law::Descend(n) => note_depth(t, path, n, v, prov),
        Law::Collections => match v.as_array() {
            Some(a) => {
                for e in a {
                    path.push(collection_seg(e));
                    note_table(t, path, &Collection::shape(), e, prov);
                    path.pop();
                }
            }
            None => t.record(path, prov),
        },
        Law::Prepend => match v.as_array() {
            Some(a) => {
                for i in 0..a.len() {
                    path.push(index_seg(i));
                    t.record(path, prov);
                    path.pop();
                }
            }
            None => t.record(path, prov),
        },
    }
}

pub(crate) fn note_table(
    t: &mut Trace,
    path: &mut Vec<String>,
    shape: &Shape,
    v: &toml::Value,
    prov: Prov,
) {
    let Some(tbl) = v.as_table().filter(|tbl| !tbl.is_empty()) else {
        t.record(path, prov);
        return;
    };
    for (k, kv) in tbl {
        path.push(k.clone());
        note_key(t, path, law_of(shape, k), kv, prov);
        path.pop();
    }
}

fn note_depth(t: &mut Trace, path: &mut Vec<String>, depth: usize, v: &toml::Value, prov: Prov) {
    match v.as_table().filter(|tbl| depth > 0 && !tbl.is_empty()) {
        None => t.record(path, prov),
        Some(tbl) => {
            for (k, kv) in tbl {
                path.push(k.clone());
                note_depth(t, path, depth - 1, kv, prov);
                path.pop();
            }
        }
    }
}

/// Merge one table over another by each key's law (MERGE.md B2). `path`/`t`
/// record provenance (MERGE.md B3); load uses `Trace::off()`.
pub(crate) fn merge_table(
    base: toml::Value,
    site: toml::Value,
    shape: &Shape,
    path: &mut Vec<String>,
    t: &mut Trace,
) -> toml::Value {
    let (Some(bt), Some(st)) = (base.as_table(), site.as_table()) else {
        t.record(path, t.near_over());
        return site;
    };
    let mut out = bt.clone();
    for (k, sv) in st.clone() {
        path.push(k.clone());
        let law = law_of(shape, &k);
        let merged = match out.remove(&k) {
            Some(bv) => merge_by(law, bv, sv, path, t),
            None => {
                note_key(t, path, law, &sv, t.near());
                sv
            }
        };
        path.pop();
        out.insert(k, merged);
    }
    if let Some(far) = t.far().filter(|_| t.on()) {
        for (k, bv) in bt.iter().filter(|(k, _)| !st.contains_key(*k)) {
            path.push(k.clone());
            note_key(t, path, law_of(shape, k), bv, far);
            path.pop();
        }
    }
    toml::Value::Table(out)
}

/// Per-key merge down `depth` table levels; below that, site replaces whole.
pub(crate) fn merge_to_depth(
    base: toml::Value,
    site: toml::Value,
    depth: usize,
    path: &mut Vec<String>,
    t: &mut Trace,
) -> toml::Value {
    let (Some(bt), Some(st)) = (base.as_table(), site.as_table()) else {
        t.record(path, t.near_over());
        return site;
    };
    if depth == 0 {
        t.record(path, t.near_over());
        return site;
    }
    let mut out = bt.clone();
    for (k, sv) in st.clone() {
        path.push(k.clone());
        let merged = match out.remove(&k) {
            Some(bv) => merge_to_depth(bv, sv, depth - 1, path, t),
            None => {
                note_depth(t, path, depth - 1, &sv, t.near());
                sv
            }
        };
        path.pop();
        out.insert(k, merged);
    }
    if let Some(far) = t.far().filter(|_| t.on()) {
        for (k, bv) in bt.iter().filter(|(k, _)| !st.contains_key(*k)) {
            path.push(k.clone());
            note_depth(t, path, depth - 1, bv, far);
            path.pop();
        }
    }
    toml::Value::Table(out)
}

/// Collection identity across the merge: source dir, else name.
pub(crate) fn collection_key(c: &toml::Value) -> Option<String> {
    let t = c.as_table()?;
    identity(
        t.get("source").and_then(|v| v.as_str()),
        t.get("name").and_then(|v| v.as_str()),
    )
}

/// Same identity rule for TOML merge and typed post-merge (must not drift).
pub(crate) fn identity(source: Option<&str>, name: Option<&str>) -> Option<String> {
    if let Some(s) = source {
        let s = s.trim_end_matches('/');
        return Some(format!("source:{}", if s.is_empty() { "." } else { s }));
    }
    name.map(|n| format!("name:{n}"))
}

/// Diagnostic label: table name plus identity (source or rules).
pub(crate) fn describe_collection(name: &str, c: &Collection) -> String {
    match c.source.as_deref() {
        Some(s) => format!("{name:?} at `source = {s:?}`"),
        // Objects: list rule globs so two sourceless collections differ visibly.
        None => match c.rules.as_slice() {
            [] => format!("{name:?} (no `source`, no rules)"),
            rules => format!(
                "{name:?} (no `source`; rules {})",
                rules
                    .iter()
                    .map(|r| format!("{:?}", r.pattern))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        },
    }
}

fn merge_collection_list(
    base: toml::Value,
    site: toml::Value,
    path: &mut Vec<String>,
    t: &mut Trace,
) -> toml::Value {
    let (Some(ba), Some(sa)) = (base.as_array(), site.as_array()) else {
        t.record(path, t.near_over());
        return site;
    };
    let mut out = ba.clone();
    let mut paired = vec![false; ba.len()];
    for sc in sa {
        path.push(collection_seg(sc));
        match collection_key(sc).and_then(|k| {
            out.iter()
                .position(|bc| collection_key(bc).as_deref() == Some(k.as_str()))
        }) {
            Some(i) => {
                paired[i] = true;
                out[i] = merge_collection(out[i].clone(), sc.clone(), path, t);
            }
            None => {
                note_table(t, path, &Collection::shape(), sc, t.near());
                out.push(sc.clone());
            }
        }
        path.pop();
    }
    if let Some(far) = t.far().filter(|_| t.on()) {
        for bc in ba.iter().zip(&paired).filter(|(_, m)| !**m).map(|(c, _)| c) {
            path.push(collection_seg(bc));
            note_table(t, path, &Collection::shape(), bc, far);
            path.pop();
        }
    }
    toml::Value::Array(out)
}

fn merge_collection(
    base: toml::Value,
    site: toml::Value,
    path: &mut Vec<String>,
    t: &mut Trace,
) -> toml::Value {
    merge_table(base, site, &Collection::shape(), path, t)
}

/// `index.{md,html}` -> `["index.md", "index.html"]`; else one literal.
pub(crate) fn brace_alternatives(pat: &str) -> Vec<String> {
    let (Some(open), Some(close)) = (pat.find('{'), pat.find('}')) else {
        return vec![pat.to_string()];
    };
    if close < open {
        return vec![pat.to_string()];
    }
    let (head, tail) = (&pat[..open], &pat[close + 1..]);
    pat[open + 1..close]
        .split(',')
        .map(|alt| format!("{head}{}{tail}", alt.trim()))
        .collect()
}

fn default_root() -> PathBuf {
    PathBuf::from(".")
}

/// True if `content`/`default_content` has `{token}` placeholders (§5c).
/// Brace alts (`index.{md,html}`) are not tokens (comma). Malformed => true.
pub(crate) fn is_templated(s: &str) -> bool {
    grackle_db::template::tokens(s).map_or(true, |t| t.iter().any(|tok| !tok.contains(',')))
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShellDef {
    pub command: String,
}

/// A custom widget: a wrapper template plus an optional `<head>` fragment
/// pulled in on any page that uses it. `name = "<tmpl>"` is the template alone;
/// `[widgets.name] template = … head = …` carries both.
#[derive(Debug, Clone)]
pub struct WidgetDef {
    pub template: String,
    pub head: Option<String>,
}

impl<'de> Deserialize<'de> for WidgetDef {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Template(String),
            Full {
                template: String,
                head: Option<String>,
            },
        }
        Ok(match Raw::deserialize(d)? {
            Raw::Template(template) => WidgetDef {
                template,
                head: None,
            },
            Raw::Full { template, head } => WidgetDef { template, head },
        })
    }
}

/// Embed policy for unrouted rows (IO.md §4a, I11). `/static/` prefix is fixed.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmbedsCfg {
    /// Off: embed-marked rows are a load error.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Path globs narrowing the policy; empty = all embed-marked rows.
    #[serde(default, rename = "match", deserialize_with = "one_or_many_string")]
    pub patterns: Vec<String>,
}

impl Default for EmbedsCfg {
    fn default() -> Self {
        EmbedsCfg {
            enabled: true,
            patterns: Vec::new(),
        }
    }
}

impl EmbedsCfg {
    /// Compiled `match` globs; `None` = admit all (vs empty GlobSet = admit none).
    /// Case-insensitive like rule globs (IO.md I7a).
    pub fn compiled(&self) -> Result<Option<globset::GlobSet>> {
        if self.patterns.is_empty() {
            return Ok(None);
        }
        let mut b = globset::GlobSetBuilder::new();
        for p in &self.patterns {
            b.add(
                globset::GlobBuilder::new(p)
                    .case_insensitive(true)
                    .build()
                    .with_context(|| format!("bad [embeds] match glob {p:?}"))?,
            );
        }
        Ok(Some(b.build()?))
    }
}

/// Display strings for the pairing axis (§6f). Membership is on [`I18nCfg::axis`].
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct I18nCfg {
    /// Pairing axis name; default `"locale"`. Inert until `[axes.*]` declares it.
    #[serde(default = "default_i18n_axis")]
    pub axis: Option<String>,
    /// Member display names; keys must be declared axis members (C4a).
    #[serde(default)]
    pub names: BTreeMap<String, String>,
    /// Global `"@key"` map (§6f); values literal, no chains. Unused site keys error.
    #[serde(default)]
    pub strings: BTreeMap<String, LocalizedStr>,
    /// Indexed tables (§6f); whole-table atom replace (with `strings` under Descend(2)).
    #[serde(default)]
    pub tables: BTreeMap<String, I18nTable>,
}

/// `[i18n.tables.<name>]`: index -> LocalizedStr; table-spelled atom.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(transparent)]
pub struct I18nTable(BTreeMap<String, LocalizedStr>);

impl I18nTable {
    pub fn get(&self, index: &str) -> Option<&LocalizedStr> {
        self.0.get(index)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &LocalizedStr)> {
        self.0.iter()
    }
}

impl Shaped for I18nTable {
    fn shape() -> Shape {
        Shape::TableAtom
    }
}

impl Default for I18nCfg {
    fn default() -> Self {
        Self {
            axis: default_i18n_axis(),
            names: BTreeMap::new(),
            strings: BTreeMap::new(),
            tables: BTreeMap::new(),
        }
    }
}

fn default_i18n_axis() -> Option<String> {
    Some("locale".to_string())
}

impl I18nCfg {
    pub fn name_of<'a>(&'a self, locale: &'a str) -> &'a str {
        self.names.get(locale).map(String::as_str).unwrap_or(locale)
    }

    /// Missing -> `""`. `canonical` is the pairing-axis fallback for maps.
    pub fn string<'a>(&'a self, key: &str, member: &str, canonical: &str) -> &'a str {
        self.strings
            .get(key)
            .map(|s| s.get(member, canonical))
            .unwrap_or("")
    }

    pub fn table<'a>(
        &'a self,
        name: &str,
        index: &str,
        member: &str,
        canonical: &str,
    ) -> &'a str {
        self.tables
            .get(name)
            .and_then(|t| t.get(index))
            .map(|s| s.get(member, canonical))
            .unwrap_or("")
    }

    /// Inline wins; `"@key"` falls back to the global map (§6f).
    pub fn text<'a>(&'a self, s: &'a LocalizedStr, member: &str, canonical: &str) -> &'a str {
        match s.reference() {
            Some(key) => self.string(key, member, canonical),
            None => s.get(member, canonical),
        }
    }
}

/// Field declarations + embeddings/search consumers (§5b).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SchemaBag {
    #[serde(default)]
    pub embeddings: EmbeddingsSchema,
    #[serde(default)]
    pub search: SearchSchema,
    /// Flattened field decls (`draft = { type = "bool" }`, ...).
    #[serde(flatten)]
    pub fields: toml::Table,
}

/// Embed text template; `{body}` + row fields (lists join with `", "`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct EmbeddingsSchema {
    #[serde(default)]
    pub string: String,
}

/// Search index vs store fields; `body` is stripped text.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchSchema {
    #[serde(default)]
    pub index: Vec<String>,
    #[serde(default)]
    pub store: Vec<String>,
}

impl crate::shape::Shaped for SchemaBag {
    fn shape() -> crate::shape::Shape {
        // Per-key atom replace under [schema].
        crate::shape::Shape::Map(Box::new(crate::shape::Shape::Atom))
    }
}

/// Head tags and `<html>`/`<body>` attributes (§4e).
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct HtmlCfg {
    #[serde(default)]
    pub head: HeadCfg,
    /// `<html>` attrs; `lang` lives here, not as engine vocabulary.
    #[serde(default)]
    pub html: AttrCfg,
    #[serde(default)]
    pub body: AttrCfg,
}

/// Attribute name -> §5f expression; empty result omits the attr (§5e).
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct AttrCfg {
    #[serde(default)]
    pub attribute: BTreeMap<String, String>,
}

/// Head entry: CEL expr, multi-attr table, or expand over a pool (§4e).
/// Table-spelled atom under Descend(3).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum HeadEntry {
    Expr(String),
    Expand(HeadExpand),
    /// Multi-attr table; after Expand so `from` stays an expand.
    Attrs(BTreeMap<String, String>),
}

impl HeadEntry {
    pub fn as_expr(&self) -> Option<&str> {
        match self {
            HeadEntry::Expr(s) => Some(s.as_str()),
            HeadEntry::Expand(_) | HeadEntry::Attrs(_) => None,
        }
    }
}

impl PartialEq<&str> for HeadEntry {
    fn eq(&self, other: &&str) -> bool {
        self.as_expr() == Some(*other)
    }
}

impl PartialEq<str> for HeadEntry {
    fn eq(&self, other: &str) -> bool {
        self.as_expr() == Some(other)
    }
}

impl PartialEq<String> for HeadEntry {
    fn eq(&self, other: &String) -> bool {
        self.as_expr() == Some(other.as_str())
    }
}

/// One tag per member of `from`; attrs are CEL.
#[derive(Debug, Clone, Deserialize)]
pub struct HeadExpand {
    /// Candidate pool (relation / `axis.*`), same word as RelationCfg::from.
    pub from: String,
    #[serde(flatten)]
    pub attrs: BTreeMap<String, String>,
}

impl Shaped for HeadEntry {
    fn shape() -> Shape {
        Shape::TableAtom
    }
}

/// JSON-LD leaf expr or nested object; empty / no `@type` => omit (§4e).
#[derive(Debug, Clone)]
pub enum JsonLdValue {
    Expr(String),
    Object(BTreeMap<String, JsonLdValue>),
}

impl<'de> serde::Deserialize<'de> for JsonLdValue {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = toml::Value::deserialize(d)?;
        jsonld_from_toml(v).map_err(serde::de::Error::custom)
    }
}

fn jsonld_from_toml(v: toml::Value) -> Result<JsonLdValue, String> {
    match v {
        toml::Value::String(s) => Ok(JsonLdValue::Expr(s)),
        toml::Value::Table(t) => {
            let mut out = BTreeMap::new();
            for (k, v) in t {
                out.insert(k, jsonld_from_toml(v)?);
            }
            Ok(JsonLdValue::Object(out))
        }
        other => Err(format!(
            "jsonld value must be a string expression or a table, got {}",
            other.type_str()
        )),
    }
}

impl Shaped for JsonLdValue {
    fn shape() -> Shape {
        // Whole-object replace, not field compose.
        Shape::TableAtom
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct HeadCfg {
    /// `<meta name>`: §5f expr; empty omits (§5e). Names are config, not engine.
    #[serde(default)]
    pub meta: BTreeMap<String, HeadEntry>,
    /// `<meta property>` (OG / article:*); separate attr, same mechanism (§4e).
    #[serde(default)]
    pub property: BTreeMap<String, HeadEntry>,
    /// `<link rel>`: expr, attr table, or expand (§4e).
    #[serde(default)]
    pub link: BTreeMap<String, HeadEntry>,
    /// JSON-LD; nested tables -> objects; empty `@type` after eval => no script.
    #[serde(default)]
    pub jsonld: BTreeMap<String, JsonLdValue>,
}

/// Internal-link policy (§6a).
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct LinksCfg {
    #[serde(default)]
    pub policy: LinkPolicy,
}

/// `strict` (default): broken / raw internal URLs are load errors. `loose`: migrate.
#[derive(Debug, Deserialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LinkPolicy {
    Loose,
    #[default]
    Strict,
}

/// One grouped-field value: slug/name default to id; intro optional.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordCfg {
    pub slug: Option<String>,
    pub name: Option<LocalizedStr>,
    pub intro: Option<LocalizedStr>,
}

/// Display string (§6f): bare, per-member map, `"@key"`, or `"@table[index]"`.
/// Inline beats global; `"@@"` escapes a leading `@`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum LocalizedStr {
    One(String),
    PerMember(BTreeMap<String, String>),
}

impl PartialEq<&str> for LocalizedStr {
    fn eq(&self, other: &&str) -> bool {
        matches!(self, LocalizedStr::One(s) if s == *other)
    }
}

impl PartialEq<str> for LocalizedStr {
    fn eq(&self, other: &str) -> bool {
        matches!(self, LocalizedStr::One(s) if s == other)
    }
}

impl PartialEq<String> for LocalizedStr {
    fn eq(&self, other: &String) -> bool {
        matches!(self, LocalizedStr::One(s) if s == other)
    }
}

/// Prefix `"@name[index]"`; name is `[A-Za-z0-9_]`.
pub fn parse_table_ref(s: &str) -> Option<(&str, &str)> {
    let rest = s.strip_prefix('@').filter(|r| !r.starts_with('@'))?;
    let (name, after) = rest.split_once('[')?;
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return None;
    }
    let close = after.find(']')?;
    Some((name, &after[..close]))
}

/// Strip leading zeros on all-digit indexes (`"03"` -> `"3"`; `"0"` stays).
pub fn normalize_table_index(index: &str) -> String {
    if !index.is_empty() && index.bytes().all(|b| b.is_ascii_digit()) {
        let trimmed = index.trim_start_matches('0');
        if trimmed.is_empty() {
            "0".into()
        } else {
            trimmed.into()
        }
    } else {
        index.to_string()
    }
}

/// Every `@name[...]` table name in `text`, skipping `@@`.
pub fn table_ref_names(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find('@') {
        let after = &rest[at + 1..];
        if after.starts_with('@') {
            rest = &after[1..];
            continue;
        }
        if let Some((name, index)) = parse_table_ref(&rest[at..]) {
            out.push(name);
            rest = &rest[at + 1 + name.len() + 1 + index.len() + 1..];
        } else {
            rest = after;
        }
    }
    out
}

impl LocalizedStr {
    /// `"@key"` string-map ref (not `@table[...]`).
    pub fn reference(&self) -> Option<&str> {
        match self {
            LocalizedStr::One(s) => s
                .strip_prefix('@')
                .filter(|rest| !rest.starts_with('@') && !rest.contains('[')),
            LocalizedStr::PerMember(_) => None,
        }
    }

    /// Whole-value `"@table[index]"`.
    pub fn table_ref(&self) -> Option<(&str, &str)> {
        match self {
            LocalizedStr::One(s) => {
                let (name, index) = parse_table_ref(s)?;
                let span = 1 + name.len() + 1 + index.len() + 1;
                (span == s.len()).then_some((name, index))
            }
            LocalizedStr::PerMember(_) => None,
        }
    }

    pub fn table_names(&self) -> Vec<&str> {
        match self {
            LocalizedStr::One(s) if s.starts_with("@@") => Vec::new(),
            LocalizedStr::One(s) => table_ref_names(s),
            LocalizedStr::PerMember(m) => m.values().flat_map(|v| table_ref_names(v)).collect(),
        }
    }

    /// Member else canonical; reference-blind (see `I18nCfg::text`).
    pub fn get<'a>(&'a self, member: &str, canonical: &str) -> &'a str {
        match self {
            // `@@` -> `@`; keep `@table[...]` for later expansion.
            LocalizedStr::One(s) => {
                if s.starts_with("@@") {
                    s.strip_prefix('@').unwrap_or(s)
                } else if parse_table_ref(s).is_some()
                    || (s.starts_with('@') && s.contains('['))
                {
                    s
                } else {
                    s.strip_prefix('@').unwrap_or(s)
                }
            }
            LocalizedStr::PerMember(m) => m
                .get(member)
                .or_else(|| m.get(canonical))
                .map(String::as_str)
                .unwrap_or(""),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Site {
    pub url: String,
    #[serde(default)]
    pub baseurl: String,
    /// Display name (§6f): bare, per-member map, or `@key`.
    pub title: LocalizedStr,
    pub author: String,
    /// Feed `<author><email>`; omitted when absent.
    pub email: Option<String>,
    /// Site default in the theme cascade (§5a). `None` => `default/` dir or base
    /// theme (not the string `"default"`). Full `name:tokens` allowed.
    pub theme: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Collection {
    /// Override table name when source dir is the wrong word.
    pub name: Option<String>,
    /// Scope role via source (IO.md I7d): absent = objects; `"."` = tree;
    /// else posts (subtree walked, owned, and ordered in the rule sequence).
    pub source: Option<String>,
    /// Default stem extractors for this collection's rules (§4).
    #[serde(default)]
    pub file: Vec<String>,
    /// Not-content globs; read from the tree collection only (§4c, IO.md I7b).
    /// Govern the whole walk (IO.md I7d); excluding a posts `source` is refused.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Re-admit paths; same tree-only restriction as `exclude`.
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// View whose subdivision chain forms row trails.
    pub trail: Option<String>,
    /// List-field archive view names (q32); else unique grouped view.
    #[serde(default)]
    pub archives: BTreeMap<String, String>,
    /// Neighbour queries (§6g, q52); override per NAME.
    #[serde(default)]
    pub relations: BTreeMap<String, RelationCfg>,
    /// Typed fields for every row of this collection (§5b).
    #[serde(default)]
    pub schema: toml::Table,
    /// From base rather than site (§4d); errors name inherited collections.
    #[serde(skip)]
    pub inherited: bool,
}

impl Collection {
    /// Objects scope: no `source` (IO.md I7a).
    pub fn is_objects(&self) -> bool {
        self.source.is_none()
    }

    /// Tree scope: `source = "."`.
    pub fn is_tree(&self) -> bool {
        self.source.as_deref() == Some(".")
    }

    /// Posts scope: owns its subtree (IO.md I7d).
    pub fn is_posts(&self) -> bool {
        !self.is_objects() && !self.is_tree()
    }
}

/// Neighbour query over self/candidate (§6g).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelationCfg {
    /// Candidate pool (set / collection / derived); absent = this collection.
    /// Same word as View::from (MERGE.md G1).
    pub from: Option<String>,
    /// Predicate over `self`/`candidate`; absent = every candidate.
    #[serde(rename = "where")]
    pub filter: Option<String>,
    /// Which `self` rows carry this; also picks `.schema.toml` for typing (§6g).
    pub scope: Option<String>,
    /// Score, bigger wins; absent = embedding order.
    pub rank: Option<String>,
    /// Drop below this after `rank` (adjusted score, not a `where` clause).
    pub min_rank: Option<f64>,
    pub limit: Option<usize>,
    /// Heading via `@ref` into `[i18n.strings]` (default `@NAME`).
    pub label: Option<LocalizedStr>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    #[serde(rename = "match")]
    pub pattern: String,
    /// Landing template(s); list form for default-axis fallthrough (§6f).
    #[serde(default, deserialize_with = "one_or_many_string")]
    pub route: Vec<String>,
    /// Gate on front-matter presence (page vs static copy).
    pub front_matter: Option<bool>,
    /// Emit a Route only when something references the row (§4).
    #[serde(default)]
    pub on_demand: Option<bool>,
    /// No canonical URL; embed policy addresses via `/static/` (IO.md §4a, I11).
    /// Mutually exclusive with `route` and `on_demand`.
    #[serde(default)]
    pub embed: Option<bool>,
    /// Stem extractors; absent => [`Collection::file`].
    #[serde(default)]
    pub file: Vec<String>,
    #[serde(default)]
    pub defaults: BTreeMap<String, toml::Value>,
    /// From base (§4d); only site-written rules warn when dead.
    #[serde(skip)]
    pub inherited: bool,
}

/// Alternative forms of one row (q53): values + field; routes spend the axis.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Axis {
    /// Members in order; first is canonical.
    pub values: Vec<String>,
    /// Row field set while rendering each member.
    pub field: String,
}

impl Axis {
    pub fn canonical(&self) -> Option<&str> {
        self.values.first().map(String::as_str)
    }
}

/// What a view ranges over (§5c): one name or a union of collections.
/// Absent `from` on a fold = whole output pool (IO.md §4, I3).
/// Union: collections only, same kind.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum From {
    One(String),
    Union(Vec<String>),
}

impl From {
    /// `None` for a union (terminates a chain).
    pub fn single(&self) -> Option<&str> {
        match self {
            From::One(s) => Some(s.as_str()),
            From::Union(_) => None,
        }
    }

    pub fn names(&self) -> &[String] {
        match self {
            From::One(s) => std::slice::from_ref(s),
            From::Union(v) => v.as_slice(),
        }
    }

    pub fn display(&self) -> String {
        match self {
            From::One(s) => format!("{s:?}"),
            From::Union(v) => format!("{v:?}"),
        }
    }
}

/// `axis = "theme"` or list -> Vec; empty when absent.
fn one_or_many_string<'de, D>(d: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    Ok(match OneOrMany::deserialize(d)? {
        OneOrMany::One(s) => vec![s],
        OneOrMany::Many(v) => v,
    })
}

/// Query plus optional materialization (DESIGN.md §5c). Extra keys in
/// `route_fields` are checked against `[schema]` at validate.
#[derive(Debug, Deserialize)]
pub struct View {
    /// Collection, set/route, or union of collections. Absent on a fold =
    /// whole output pool (IO.md §4, I3).
    #[serde(default)]
    pub from: Option<From>,
    /// Predicate including path scoping via `glob(path, ...)` (MERGE.md G2).
    #[serde(rename = "where")]
    pub filter: Option<String>,
    /// Explicit order (`-` = descending); not defaulted.
    pub order_by: Option<String>,
    pub group_by: Option<String>,
    pub paginate: Option<usize>,
    /// Present => `[routes]`; absent => `[sets]`.
    #[serde(rename = "path")]
    pub route: Option<String>,
    #[serde(default, rename = "paths")]
    pub routes: Vec<String>,
    pub layout: Option<String>,
    /// Theme fragment variant (q24): `{kind}--{variant}.html`.
    pub variant: Option<String>,
    /// Axes this route materializes across (q53).
    #[serde(default, deserialize_with = "one_or_many_string")]
    pub axis: Vec<String>,
    /// Theme for this view (`name[:tokens]`); nearest beats member unanimity (§5a).
    pub theme: Option<String>,
    /// Pairing-axis partition; default-on. `"default"` opts out; `"*"` = all (§6f).
    pub partition: Option<String>,
    /// Outermost serialization: `atom` / `sitemap` / `search` / registered (§5g).
    pub shell: Option<String>,
    pub limit: Option<usize>,
    pub template: Option<String>,
    /// Listing title template over group params / `@table[...]`.
    pub title: Option<LocalizedStr>,
    /// Breadcrumb label; defaults to `title`.
    pub crumb: Option<LocalizedStr>,
    /// Mode A landing prose (q45).
    pub intro: Option<LocalizedStr>,
    /// Mode B: claim this source path as the landing body (q45).
    pub content: Option<String>,
    /// Mode B if the row exists; else plain landing. Lets base ship `/` (§4d).
    pub default_content: Option<String>,
    /// From base (§4d); inherited empty routes do not materialize.
    #[serde(skip)]
    pub inherited: bool,
    /// Declared under `[sets]` (true) vs `[routes]`; not derived from path
    /// (default_content may clear path) (§4a, MERGE.md C6c).
    #[serde(skip)]
    pub declared_set: bool,
    /// Profile that wrote this view's filter (MERGE.md C6b).
    #[serde(skip)]
    pub filter_profile: Option<String>,
    /// Computed columns; flow through `from` composition (§6d / §5f).
    #[serde(default)]
    pub fields: BTreeMap<String, Field>,
    /// Schema-declared route answers as top-level keys (§4e).
    #[serde(default, flatten)]
    pub route_fields: BTreeMap<String, toml::Value>,
}

/// Computed field: TOML string = CEL expr; other types = literals (§5f / §6d).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Field {
    Expr(String),
    Value(toml::Value),
}

impl Field {
    pub fn as_expr(&self) -> Option<&str> {
        match self {
            Field::Expr(s) => Some(s.as_str()),
            Field::Value(_) => None,
        }
    }
}

impl View {
    /// Query-only: nothing to inherit ambiguously; only such views may be `from`.
    pub fn is_query_only(&self) -> bool {
        self.route.is_none()
            && self.routes.is_empty()
            && self.layout.is_none()
            && self.template.is_none()
            && self.paginate.is_none()
            && self.limit.is_none()
            && self.group_by.is_none()
    }

    pub fn is_materialized(&self) -> bool {
        self.route.is_some() || !self.routes.is_empty()
    }

    /// Fold over every output: no `from` (IO.md §4, I3).
    pub fn reads_all_outputs(&self) -> bool {
        self.from.is_none()
    }
}

/// Flattened `from` chain.
#[derive(Debug)]
pub struct Query {
    /// Collections ranged over; empty = fold over every output (IO.md §4).
    pub base: Vec<String>,
    /// Filters along the chain, outermost last; all must hold (MERGE.md G2).
    pub filters: Vec<String>,
    /// Nearest `order_by` (nearest wins).
    pub order_by: Option<String>,
    /// Attribution for profile-replaced `where` clauses (MERGE.md C6a).
    pub patched: Vec<String>,
}

impl Query {
    pub fn predicate(&self) -> Option<String> {
        match self.filters.len() {
            0 => None,
            1 => Some(self.filters[0].clone()),
            _ => Some(
                self.filters
                    .iter()
                    .map(|f| format!("({f})"))
                    .collect::<Vec<_>>()
                    .join(" && "),
            ),
        }
    }
}

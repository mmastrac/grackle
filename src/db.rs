//! The database: tables, indexes, views, and the constraints that make a bad
//! site a load-time error instead of a 404. See DESIGN.md §3, §4, §5.

use anyhow::{bail, Context, Result};
use chrono::NaiveDate;
use globset::{Glob, GlobMatcher, GlobSet, GlobSetBuilder};
use rayon::prelude::*;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use crate::config::{Collection, Config, Kind};
use crate::filter;
use crate::markers::Markers;
use crate::route::{self, FilenameFormat};
use crate::store::{self, RawRow};

// ------------------------------------------------------------------ rows

#[derive(Debug, Default, Serialize)]
pub struct Post {
    pub path: PathBuf,
    pub rel: PathBuf,
    #[serde(serialize_with = "hex")]
    pub version: u64,
    pub date: Option<NaiveDate>,
    pub slug: String,
    /// Filename without extension — unique, because it carries the date.
    pub stem: String,
    /// Source path relative to the collection, without the extension:
    /// `2009/2009-07-28-a-quieter-window-name-transport-for-ie`.
    ///
    /// This is the key `{% post_url %}` actually takes. Measured: all 51 uses
    /// in the corpus are this `dir/stem` form, none are a bare stem — because
    /// posts live in year subdirectories.
    pub name: String,
    pub title: String,
    pub description: Option<String>,
    pub layout: Option<String>,
    pub tags: Vec<String>,
    pub draft: bool,
    pub hidden: bool,
    pub noindex: bool,
    /// Render the heading outline (§6e); front matter or cascaded default.
    pub toc: bool,
    /// The locale axis (§6f): assigned by the path selector at load.
    pub locale: String,
    /// The locale-stripped identity shared by a row and its translations
    /// (collection-relative, no extension). Pairing key for `by_logical`.
    #[serde(skip)]
    pub logical: String,
    pub url: String,
    pub body_bytes: usize,
    #[serde(skip)]
    pub body: String,
}

impl Post {
    pub fn year_month(&self) -> Option<(i32, u32)> {
        use chrono::Datelike;
        self.date.map(|d| (d.year(), d.month()))
    }
}

impl Page {
    /// The hero image source (q23): the explicit `cover:` field beats
    /// `image:`; both must be image-typed schema fields (§5b). The
    /// first-image-block fallback remains open.
    pub fn hero_source(&self) -> Option<&str> {
        self.images
            .get("cover")
            .or_else(|| self.images.get("image"))
            .map(String::as_str)
    }
}

/// `2022-03-16` — sortable; the machine-readable date everywhere.
pub fn iso_date(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// `16 March 2022` — what themes and search hits show.
pub fn pretty_date(d: NaiveDate) -> String {
    d.format("%-d %B %Y").to_string()
}

/// A tree row: rendered page (has front matter) or static file (does not).
#[derive(Debug, Serialize)]
pub struct Page {
    pub path: PathBuf,
    pub rel: PathBuf,
    #[serde(serialize_with = "hex")]
    pub version: u64,
    pub url: String,
    pub rendered: bool,
    pub size: u64,
    /// Schema, for rendered rows only (§5a). `layout` is the legacy field that
    /// now selects a *theme* plus a layout kind.
    pub title: Option<String>,
    pub layout: Option<String>,
    pub description: Option<String>,
    /// Declared position within a section tree (§6e).
    pub order: Option<i64>,
    /// Render the heading outline (§6e).
    pub toc: bool,
    /// Which theme renders this row (§5a) — front matter beats rule
    /// defaults; None means the site default.
    pub theme: Option<String>,
    /// Typed extra fields, validated against the governing `.schema.toml`
    /// (§5b). Empty for ungoverned rows.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, filter::Value>,
    /// The image-typed subset: field name → root-relative source path.
    /// Feeds the thumb pass and the `hero` part (q23).
    #[serde(skip)]
    pub images: BTreeMap<String, String>,
    /// The locale axis (§6f): assigned by the path selector at load.
    pub locale: String,
    /// The locale-stripped identity shared by a row and its translations.
    #[serde(skip)]
    pub logical: String,
}

#[derive(Debug, Serialize)]
pub struct Object {
    pub path: PathBuf,
    pub rel: PathBuf,
    #[serde(serialize_with = "hex")]
    pub version: u64,
    pub url: String,
    pub ext: String,
    pub name: String,
    pub size: u64,
}

fn hex<S: serde::Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&format!("{v:016x}"))
}

// ------------------------------------------------------------------ tables

#[derive(Debug, Default, Serialize)]
pub struct PostsTable {
    pub rows: Vec<Post>,
    /// Reverse-chronological over the dated set; undated rows sort last.
    #[serde(skip)]
    pub order: Vec<usize>,
    /// The primary index (DESIGN.md §3): `(date, slug)`, unique.
    /// NOT `slug` alone — measured: `not-dead-yet` is used by both a 2003 and
    /// a 2006 post, which is legal because their dates (and so URLs) differ.
    #[serde(skip)]
    pub by_key: HashMap<(Option<NaiveDate>, String), usize>,
    /// `dir/stem` -> row, for `{% post_url %}`. Unique.
    #[serde(skip)]
    pub by_name: HashMap<String, usize>,
    /// Non-unique: slug -> rows. Informational; see `by_key` for identity.
    #[serde(skip)]
    pub by_slug: BTreeMap<String, Vec<usize>>,
    #[serde(skip)]
    pub by_tag: BTreeMap<String, Vec<usize>>,
    #[serde(skip)]
    pub by_year_month: BTreeMap<(i32, u32), Vec<usize>>,
    #[serde(skip)]
    pub by_url: HashMap<String, usize>,
    /// §6f: logical identity -> every locale variant (default included).
    /// The ONLY index that sees translations — `order`/`by_key`/`by_tag`/
    /// `by_year_month` admit default-locale rows only, which is what keeps
    /// every listing, feed and archive single-locale by construction.
    #[serde(skip)]
    pub by_logical: HashMap<String, Vec<usize>>,
}

impl PostsTable {
    /// Adjacency over `order`. Returns (newer, older) as index into `rows`.
    pub fn neighbors(&self, idx: usize) -> (Option<usize>, Option<usize>) {
        let Some(pos) = self.order.iter().position(|&i| i == idx) else {
            return (None, None);
        };
        let newer = if pos > 0 { Some(self.order[pos - 1]) } else { None };
        let older = self.order.get(pos + 1).copied();
        (newer, older)
    }
}

#[derive(Debug, Default, Serialize)]
pub struct TreeTable {
    pub rows: Vec<Page>,
    /// §6f: logical identity -> every locale variant, rendered rows only.
    #[serde(skip)]
    pub by_logical: HashMap<String, Vec<usize>>,
}

#[derive(Debug, Default, Serialize)]
pub struct ObjectsTable {
    pub rows: Vec<Object>,
    /// Deliberately non-unique (DESIGN.md §6a).
    #[serde(skip)]
    pub by_name: BTreeMap<String, Vec<usize>>,
}

// ------------------------------------------------------------------ routes

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RouteKind {
    Post,
    Page,
    Static,
    Object,
    View,
}

impl RouteKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RouteKind::Post => "post",
            RouteKind::Page => "page",
            RouteKind::Static => "static",
            RouteKind::Object => "object",
            RouteKind::View => "view",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Route {
    pub url: String,
    pub kind: RouteKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<usize>,
    /// 1-based page number for paginated views; None otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<usize>,
    /// Group-key parameters for view routes (`year`, `month`, `key`, …),
    /// accumulated along the subdivision chain (§5c). Presentation renders
    /// the view's `title`/`crumb` templates from these.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<(String, String)>,
    /// §6f: set only for non-default-locale rows (a translation's route);
    /// None means the default locale, and filters see Null.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    /// The source row's flags, carried onto the route so a `*` view (the
    /// sitemap) can exclude them. Only post rows can be flagged, so this is
    /// false for every other route. Without it the sitemap's filter language
    /// has no way to say "not a draft" and a future draft would leak into the
    /// most public URL there is (DESIGN.md §4a).
    #[serde(skip)]
    pub draft: bool,
    #[serde(skip)]
    pub hidden: bool,
    /// `self`: the post rows this route materializes, in order.
    ///
    /// The view's declared query decides these once, here. Before this existed
    /// `build.rs` re-derived them with a `match` on the view's *name* — the
    /// config declared `filter`/`group_by`/`paginate` and the renderer ignored
    /// all of it and reimplemented each view by hand. Empty for `over = "*"`
    /// views, which range over routes rather than posts.
    #[serde(skip)]
    pub members: Vec<usize>,
}

impl Route {
    /// A route with nothing but a URL and a kind — the base every
    /// constructor site fills its few meaningful fields over.
    pub(crate) fn new(url: String, kind: RouteKind) -> Route {
        Route {
            url,
            kind,
            source: None,
            view: None,
            key: None,
            rows: None,
            page: None,
            params: Vec::new(),
            locale: None,
            draft: false,
            hidden: false,
            members: Vec::new(),
        }
    }

    /// Served as a directory (URL ends in `/`), so its output is an index.html.
    fn is_dir(&self) -> bool {
        self.url.ends_with('/')
    }

    /// Extension of the URL's last segment, or "" if it has none.
    ///
    /// Note this is "" for BOTH a directory URL and an extensionless file such
    /// as `/code/legacy/nnet/nnet` — which is exactly why `dir` is a separate
    /// field. Collapsing the two let four extensionless binaries masquerade as
    /// pages and land in the sitemap.
    fn ext(&self) -> &str {
        if self.is_dir() {
            return "";
        }
        let last = self.url.rsplit('/').next().unwrap_or("");
        match last.rsplit_once('.') {
            Some((_, e)) => e,
            None => "",
        }
    }
}

/// Fields a filter may reference on a route.
///
/// Deliberately does NOT expose `noindex`: it would need the layout chain
/// (phase 2) to compute, and a field we cannot populate correctly is worse than
/// no field — referencing it is a load-time error instead of a silent lie.
/// Empirically it is not needed anyway: jekyll-sitemap ignores noindex.
pub fn route_schema() -> filter::Schema {
    use filter::Type::*;
    let mut s = filter::Schema::new();
    s.insert("kind", Str);
    s.insert("view", Str);
    s.insert("url", Str);
    s.insert("ext", Str);
    s.insert("dir", Bool);
    s.insert("draft", Bool);
    s.insert("hidden", Bool);
    s.insert("key", Str);
    s.insert("page", Int);
    s.insert("rows", Int);
    // The source filename's stem, Null for sourceless (view) routes — the
    // same field page filters use, so `stem != "index"` says the same
    // thing at both layers (e.g. keeping listing-shaped index pages out
    // of a search shell's row set).
    s.insert("stem", Str);
    // §6f: the row's locale for translation routes; Null for the default
    // locale (and every sourceless route). `locale != "fr"` keeps French
    // rows out of a star view; Null passes `!=` by the filter's rule.
    s.insert("locale", Str);
    s
}

impl filter::Row for Route {
    fn field(&self, name: &str) -> filter::Value {
        use filter::Value as V;
        match name {
            "kind" => V::Str(self.kind.as_str().to_string()),
            "view" => match &self.view {
                Some(v) => V::Str(v.clone()),
                None => V::Null,
            },
            "url" => V::Str(self.url.clone()),
            "ext" => V::Str(self.ext().to_string()),
            "dir" => V::Bool(self.is_dir()),
            "draft" => V::Bool(self.draft),
            "hidden" => V::Bool(self.hidden),
            "key" => match &self.key {
                Some(k) => V::Str(k.clone()),
                None => V::Null,
            },
            "page" => self.page.map_or(V::Null, |p| V::Int(p as i64)),
            "rows" => self.rows.map_or(V::Null, |r| V::Int(r as i64)),
            "stem" => self
                .source
                .as_deref()
                .and_then(|p| p.file_stem())
                .map_or(V::Null, |s| {
                    // §6f: the logical stem — a suffix-selected locale is
                    // not part of a row's identity (`index.fr` is `index`).
                    let s = s.to_string_lossy();
                    let logical = match &self.locale {
                        Some(l) => s
                            .strip_suffix(l.as_str())
                            .and_then(|rest| rest.strip_suffix('.'))
                            .unwrap_or(s.as_ref()),
                        None => s.as_ref(),
                    };
                    V::Str(logical.to_owned())
                }),
            "locale" => match &self.locale {
                Some(l) => V::Str(l.clone()),
                None => V::Null,
            },
            _ => V::Null,
        }
    }
}

#[derive(Debug, Default, Serialize)]
pub struct SiteDb {
    pub posts: PostsTable,
    pub pages: TreeTable,
    pub objects: ObjectsTable,
    pub routes: Vec<Route>,
    /// Row sets for views that resolve to exactly one — the ones with no route
    /// to hang `members` on: named queries (`published`) and embedded views
    /// (`latest`). Grouped and paginated views resolve to many sets, which live
    /// on their routes instead (DESIGN.md §5c).
    pub views: BTreeMap<String, ViewRows>,
    /// Root-relative directories containing a `.section` scope marker (§6e):
    /// each roots a section tree its rendered rows carry. Engine vocabulary
    /// like `.slots/` — no config entry names it.
    pub sections: Vec<PathBuf>,
    /// Per-subtree field declarations (§5b), from `.schema.toml` files.
    #[serde(skip)]
    pub schemas: crate::schema::Schemas,
    pub stats: LoadStats,
}

/// A routeless view's resolved rows.
#[derive(Debug, Serialize)]
pub struct ViewRows {
    /// None means query-only: a named set, not something renderable.
    pub layout: Option<String>,
    /// Fragment variant (q24), for embedded rendering.
    pub variant: Option<String>,
    pub rows: usize,
    /// Which table `members` index — embedded views span collections now
    /// (`{% view latest_recipes %}` ranges over pages).
    #[serde(skip)]
    pub table: Kind,
    #[serde(skip)]
    pub members: Vec<usize>,
}

impl Default for ViewRows {
    fn default() -> Self {
        ViewRows {
            layout: None,
            variant: None,
            rows: 0,
            table: Kind::Posts,
            members: Vec::new(),
        }
    }
}

#[derive(Debug, Default, Serialize)]
pub struct LoadStats {
    pub markers: usize,
    pub markers_ms: f64,
    pub read_ms: f64,
    pub index_ms: f64,
    pub views_ms: f64,
}

// ------------------------------------------------------------------ rules

struct CompiledRule<'a> {
    matcher: GlobMatcher,
    route: Option<&'a str>,
    front_matter: Option<bool>,
    defaults: &'a BTreeMap<String, toml::Value>,
}

fn compile_rules(c: &Collection) -> Result<Vec<CompiledRule<'_>>> {
    c.rules
        .iter()
        .map(|r| {
            Ok(CompiledRule {
                matcher: Glob::new(&r.pattern)
                    .with_context(|| format!("bad rule glob {:?}", r.pattern))?
                    .compile_matcher(),
                route: r.route.as_deref(),
                front_matter: r.front_matter,
                defaults: &r.defaults,
            })
        })
        .collect()
}

/// First-writer-wins per key (DESIGN.md §4).
fn apply_rules<'a>(
    rules: &'a [CompiledRule<'a>],
    rel: &Path,
    has_front_matter: bool,
) -> (Option<&'a str>, BTreeMap<&'a str, &'a toml::Value>) {
    let mut route: Option<&str> = None;
    let mut defaults: BTreeMap<&str, &toml::Value> = BTreeMap::new();
    for rule in rules {
        if let Some(want) = rule.front_matter {
            if want != has_front_matter {
                continue;
            }
        }
        if !rule.matcher.is_match(rel) {
            continue;
        }
        if route.is_none() {
            if let Some(r) = rule.route {
                route = Some(r);
            }
        }
        for (k, v) in rule.defaults {
            defaults.entry(k.as_str()).or_insert(v);
        }
    }
    (route, defaults)
}

fn as_bool(defaults: &BTreeMap<&str, &toml::Value>, key: &str) -> bool {
    defaults.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

fn build_globset(pats: &[String]) -> Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for p in pats {
        b.add(Glob::new(p).with_context(|| format!("bad glob {p:?}"))?);
    }
    Ok(b.build()?)
}

/// `{dir}`, `{stem}`, `{name}`, `{path}`, `{ext}` for a tree/object row.
fn path_tokens(rel: &Path, k: &str) -> Option<String> {
    let path = rel.to_string_lossy().to_string();
    match k {
        "path" => Some(path),
        "dir" => Some(
            rel.parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
        ),
        "stem" => rel.file_stem().map(|s| s.to_string_lossy().to_string()),
        "name" => rel.file_name().map(|s| s.to_string_lossy().to_string()),
        "ext" => rel.extension().map(|s| s.to_string_lossy().to_string()),
        _ => None,
    }
}

/// Collapse `//` that arise when `{dir}` is empty at the root.
fn tidy(url: String) -> String {
    let mut out = String::with_capacity(url.len());
    let mut prev_slash = false;
    for ch in url.chars() {
        if ch == '/' {
            if prev_slash {
                continue;
            }
            prev_slash = true;
        } else {
            prev_slash = false;
        }
        out.push(ch);
    }
    if !out.starts_with('/') {
        out.insert(0, '/');
    }
    out
}

// ------------------------------------------------------------------ posts

fn build_posts(
    cfg: &Config,
    name: &str,
    c: &Collection,
    markers: &Markers,
) -> Result<(PostsTable, f64, f64)> {
    let root = cfg.root();
    let source = root.join(
        c.source
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("collection {name} has kind=posts but no source"))?,
    );

    let t0 = std::time::Instant::now();
    let raws: Vec<RawRow> = store::load_dir(&source, &["md", "markdown"])?;
    let read_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let t1 = std::time::Instant::now();
    let formats: Vec<FilenameFormat> = c
        .filename_formats
        .iter()
        .map(|f| FilenameFormat::compile(f))
        .collect::<Result<_>>()?;
    if formats.is_empty() {
        bail!("collection {name} has kind=posts but no filename_formats");
    }
    let rules = compile_rules(c)?;

    let mut rows: Vec<Post> = Vec::with_capacity(raws.len());
    for raw in raws {
        // §6f: the path selector strips the locale first, so filename
        // parsing, rules and routing all see the logical path — a
        // translation rides the same machinery as its original.
        let (logical_rel, locale) = cfg.i18n.split(&raw.rel);
        let stem: String = logical_rel
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();

        // `{% post_url %}` keys on the collection-relative path minus extension.
        let name = raw
            .rel
            .with_extension("")
            .to_string_lossy()
            .to_string();
        let logical = logical_rel.with_extension("").to_string_lossy().to_string();
        let key = formats.iter().find_map(|f| f.parse(&stem));
        let date = match &key {
            Some(k) => Some(
                NaiveDate::from_ymd_opt(k.year, k.month, k.day).with_context(|| {
                    format!("{} has an impossible date in its filename", raw.path.display())
                })?,
            ),
            None => None,
        };
        let slug = key
            .as_ref()
            .map(|k| k.slug.clone())
            .unwrap_or_else(|| stem.clone());

        let (route_tmpl, rule_defaults) = apply_rules(&rules, &logical_rel, true);
        // Precedence (§4b): front matter > nearest marker > rule. Markers are
        // inserted first so `or_insert` cannot let a rule override them.
        let root_rel = raw.path.strip_prefix(&root).unwrap_or(&raw.rel).to_path_buf();
        let mut defaults: BTreeMap<&str, &toml::Value> = BTreeMap::new();
        let marker_defaults = markers.defaults_for(&root_rel);
        for (k, v) in &marker_defaults {
            defaults.insert(k.as_str(), v);
        }
        for (k, v) in rule_defaults {
            defaults.entry(k).or_insert(v);
        }
        let title = raw
            .front
            .title
            .clone()
            .unwrap_or_else(|| slug.replace('-', " "));
        let draft = raw.front.draft.unwrap_or_else(|| as_bool(&defaults, "draft"));
        let hidden = raw
            .front
            .hidden
            .unwrap_or_else(|| as_bool(&defaults, "hidden"));
        let noindex = raw
            .front
            .noindex
            .unwrap_or_else(|| as_bool(&defaults, "noindex"));
        let toc = raw.front.toc.unwrap_or_else(|| as_bool(&defaults, "toc"));
        let layout = raw.front.layout.clone().or_else(|| {
            defaults
                .get("layout")
                .and_then(|v| v.as_str())
                .map(String::from)
        });

        let url = if let Some(p) = &raw.front.permalink {
            p.clone()
        } else {
            let tmpl = route_tmpl.ok_or_else(|| {
                anyhow::anyhow!("no rule supplies a route for {}", raw.path.display())
            })?;
            if date.is_none() {
                let needs: Vec<String> = route::tokens(tmpl)
                    .into_iter()
                    .filter(|t| matches!(t.as_str(), "year" | "month" | "day"))
                    .collect();
                if !needs.is_empty() {
                    bail!(
                        "{} has no date (filename doesn't match any filename_formats), \
                         but its route {:?} requires {{{}}}",
                        raw.path.display(),
                        tmpl,
                        needs.join("}, {")
                    );
                }
            }
            route::render(tmpl, |k| match k {
                "year" => date.map(|d| d.format("%Y").to_string()),
                "month" => date.map(|d| d.format("%-m").to_string()),
                "day" => date.map(|d| d.format("%-d").to_string()),
                "slug" => Some(slug.clone()),
                _ => None,
            })
            .with_context(|| format!("routing {}", raw.path.display()))?
        };
        // §6f: a translation lands at the locale-prefixed twin of its
        // original's URL.
        let url = if locale != cfg.i18n.default { format!("/{locale}{url}") } else { url };

        rows.push(Post {
            path: raw.path,
            rel: raw.rel,
            version: raw.version,
            date,
            slug,
            stem,
            name,
            title,
            description: raw.front.description,
            layout,
            tags: raw.front.tags,
            draft,
            hidden,
            noindex,
            toc,
            locale,
            logical,
            url,
            body_bytes: raw.body.len(),
            body: raw.body,
        });
    }

    rows.sort_by(|a, b| a.path.cmp(&b.path));

    // §6f: `order` drives views, feeds, archives and adjacency — it admits
    // the default locale only, which makes every one of them single-locale
    // in one place. Translations render as pages and live in `by_logical`.
    let mut order: Vec<usize> =
        (0..rows.len()).filter(|&i| rows[i].locale == cfg.i18n.default).collect();
    order.sort_by(|&a, &b| {
        match (rows[a].date, rows[b].date) {
            (Some(x), Some(y)) => y.cmp(&x),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
        .then_with(|| rows[a].slug.cmp(&rows[b].slug))
    });

    let mut table = PostsTable {
        order,
        ..Default::default()
    };

    for (i, p) in rows.iter().enumerate() {
        // Identity indexes span all locales: URLs are globally unique, and
        // `name` (physical path) keeps `{% post_url %}` unambiguous.
        if let Some(prev) = table.by_name.insert(p.name.clone(), i) {
            bail!(
                "duplicate post name {:?} ({{% post_url %}} would be ambiguous):\n  {}\n  {}",
                p.name,
                rows[prev].path.display(),
                p.path.display()
            );
        }
        if let Some(prev) = table.by_url.insert(p.url.clone(), i) {
            bail!(
                "route collision at {}:\n  {}\n  {}",
                p.url,
                rows[prev].path.display(),
                p.path.display()
            );
        }
        table.by_logical.entry(p.logical.clone()).or_default().push(i);
        // Query indexes are single-locale, like `order` (§6f): a
        // translation shares its original's (date, slug) by design.
        if p.locale != cfg.i18n.default {
            continue;
        }
        if let Some(prev) = table.by_key.insert((p.date, p.slug.clone()), i) {
            bail!(
                "duplicate (date, slug) key ({}, {:?}):\n  {}\n  {}",
                p.date.map(|d| d.to_string()).unwrap_or("none".into()),
                p.slug,
                rows[prev].path.display(),
                p.path.display()
            );
        }
        table.by_slug.entry(p.slug.clone()).or_default().push(i);
        for t in &p.tags {
            table.by_tag.entry(t.clone()).or_default().push(i);
        }
        if let Some(ym) = p.year_month() {
            table.by_year_month.entry(ym).or_default().push(i);
        }
    }

    table.rows = rows;
    let index_ms = t1.elapsed().as_secs_f64() * 1000.0;
    Ok((table, read_ms, index_ms))
}

// ------------------------------------------------------- tree + objects

/// One walk of the site root, partitioned by membership precedence
/// (DESIGN.md §3): objects win by extension, tree takes the rest.
fn build_tree_and_objects(
    cfg: &Config,
    tree_c: Option<&Collection>,
    obj_c: Option<&Collection>,
    markers: &Markers,
    schemas: &crate::schema::Schemas,
) -> Result<(TreeTable, ObjectsTable)> {
    let Some(tree_c) = tree_c else {
        return Ok((TreeTable::default(), ObjectsTable::default()));
    };
    let root = cfg.root();
    let exclude = build_globset(&tree_c.exclude)?;
    let include = build_globset(&tree_c.include)?;
    let files = store::walk_tree(&root, &exclude, &include, cfg.gitignore)?;

    // A file claimed as a view's template is not independently routable: the
    // view owns its routes. (`blog/index.html` is rendered once per paginated
    // page; `atom.xml` is the feed.)
    let templates: Vec<PathBuf> = cfg
        .views
        .values()
        .filter_map(|v| v.template.as_ref())
        .map(PathBuf::from)
        .collect();
    let files: Vec<_> = files
        .into_iter()
        .filter(|f| !templates.iter().any(|t| *t == f.rel))
        // A marker declares defaults; it is not itself content.
        .filter(|f| !markers.is_marker(&f.path))
        .collect();

    let obj_exts: Vec<String> = obj_c
        .map(|c| c.extensions.iter().map(|e| e.to_lowercase()).collect())
        .unwrap_or_default();
    let tree_rules = compile_rules(tree_c)?;
    let obj_rules = obj_c.map(compile_rules).transpose()?.unwrap_or_default();

    let is_obj = |rel: &Path| {
        let ext = rel
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        obj_exts.iter().any(|e| *e == ext)
    };

    // Only text rows can carry front matter, and only non-objects need the
    // page/static decision — so skip the peek for the ~800 binaries and run the
    // rest in parallel. (Sequential-over-everything cost ~140ms.)
    let mut files = files;
    files.par_iter_mut().for_each(|f| {
        if !is_obj(&f.rel) {
            f.has_front_matter = store::peek_front_matter(&f.path);
        }
    });

    let mut pages = TreeTable::default();
    let mut objects = ObjectsTable::default();

    for f in files {
        let ext = f
            .rel
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let is_object = is_obj(&f.rel);

        // §6f: rendered pages carry the locale axis; objects (images) are
        // shared across locales and skip the selector.
        let (logical_rel, locale) = if is_object {
            (f.rel.clone(), cfg.i18n.default.clone())
        } else {
            cfg.i18n.split(&f.rel)
        };

        let rules = if is_object { &obj_rules } else { &tree_rules };
        let (tmpl, defaults) = apply_rules(rules, &logical_rel, f.has_front_matter);
        let Some(tmpl) = tmpl else {
            bail!("no rule supplies a route for {}", f.path.display());
        };
        let url = tidy(
            route::render(tmpl, |k| path_tokens(&logical_rel, k))
                .with_context(|| format!("routing {}", f.path.display()))?,
        );
        let url =
            if locale != cfg.i18n.default { format!("/{locale}{url}") } else { url };

        if is_object {
            let name = f
                .rel
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            objects
                .by_name
                .entry(name.clone())
                .or_default()
                .push(objects.rows.len());
            objects.rows.push(Object {
                path: f.path,
                rel: f.rel,
                version: f.version,
                url,
                ext,
                name,
                size: f.size,
            });
        } else {
            // Only rendered rows have schema; 41 files, so parsing is cheap.
            let fm = if f.has_front_matter {
                read_page_schema(&f.path)?
            } else {
                Default::default()
            };
            // §5b: a governed row's extra front matter is validated — an
            // undeclared key or wrong type fails the load naming the file.
            // Ungoverned rows stay as tolerant as they always were. Schema
            // governance follows the LOGICAL path (§6f): a translation is
            // governed by the same .schema.toml as its original.
            let parent = logical_rel.parent().unwrap_or(Path::new("")).to_path_buf();
            let checked = match schemas.resolve(&parent) {
                Some(schema) if f.has_front_matter => {
                    crate::schema::validate(&schema, &fm.extra, &f.path)?
                }
                _ => Default::default(),
            };
            // Theme is chosen per row (§5a): front matter beats the rule
            // default, so one rule can restyle a subtree.
            let theme = fm.theme.clone().or_else(|| {
                defaults
                    .get("theme")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            });
            let logical = logical_rel.to_string_lossy().to_string();
            if f.has_front_matter {
                pages.by_logical.entry(logical.clone()).or_default().push(pages.rows.len());
            }
            pages.rows.push(Page {
                path: f.path,
                rel: f.rel,
                version: f.version,
                url,
                rendered: f.has_front_matter,
                size: f.size,
                title: fm.title,
                layout: fm.layout,
                description: fm.description,
                order: fm.order,
                toc: fm.toc.unwrap_or(false),
                theme,
                fields: checked.values,
                images: checked.images,
                locale,
                logical,
            });
        }
    }
    Ok((pages, objects))
}

/// Front matter of a tree page: presentation reads its fields directly.
/// A parse failure is a LOAD ERROR naming the file — this used to swallow
/// bad YAML into an empty schema, and an unquoted `title: A: B` shipped a
/// silently titleless page. Loud beats lenient (§4's constraint ethos).
fn read_page_schema(path: &Path) -> Result<crate::store::FrontMatter> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let (yaml, _) = store::split_front_matter(&text);
    serde_yaml_ng::from_str(yaml)
        .with_context(|| format!("front matter of {}", path.display()))
}

// ------------------------------------------------------------------ views

/// Fields a filter may reference on a post, and their types. Everything else is
/// a load-time error (filter.rs), so a typo can't silently match everything.
pub fn post_schema() -> filter::Schema {
    use filter::Type::*;
    let mut s = filter::Schema::new();
    s.insert("draft", Bool);
    s.insert("hidden", Bool);
    // Complete for posts: markers + front matter are the only sources (no post
    // layout sets noindex). Still absent from route_schema — see DESIGN.md §4b.
    s.insert("noindex", Bool);
    s.insert("title", Str);
    s.insert("slug", Str);
    s.insert("stem", Str);
    s.insert("layout", Str);
    s.insert("description", Str);
    s.insert("url", Str);
    // ISO-8601, so string ordering is date ordering.
    s.insert("date", Str);
    s.insert("year", Int);
    s.insert("month", Int);
    s.insert("day", Int);
    s.insert("body_bytes", Int);
    s.insert("tags", List);
    // §6f: the row's locale, always set (the default when no selector fired).
    s.insert("locale", Str);
    s
}

impl filter::Row for Post {
    fn field(&self, name: &str) -> filter::Value {
        use chrono::Datelike;
        use filter::Value as V;
        let opt_str = |o: &Option<String>| match o {
            Some(s) => V::Str(s.clone()),
            None => V::Null,
        };
        match name {
            "draft" => V::Bool(self.draft),
            "hidden" => V::Bool(self.hidden),
            "noindex" => V::Bool(self.noindex),
            "title" => V::Str(self.title.clone()),
            "slug" => V::Str(self.slug.clone()),
            "stem" => V::Str(self.stem.clone()),
            "url" => V::Str(self.url.clone()),
            "layout" => opt_str(&self.layout),
            "description" => opt_str(&self.description),
            "date" => match self.date {
                Some(d) => V::Str(d.format("%Y-%m-%d").to_string()),
                None => V::Null,
            },
            "year" => self.date.map_or(V::Null, |d| V::Int(d.year() as i64)),
            "month" => self.date.map_or(V::Null, |d| V::Int(d.month() as i64)),
            "day" => self.date.map_or(V::Null, |d| V::Int(d.day() as i64)),
            "body_bytes" => V::Int(self.body_bytes as i64),
            "tags" => V::List(self.tags.clone()),
            "locale" => V::Str(self.locale.clone()),
            _ => V::Null,
        }
    }
}

/// Fields a filter may reference on a page (tree) row. Base fields only:
/// per-subtree schema fields (§5b) vary by directory, so they join through
/// `order_by` and — later — a per-view environment, not this global schema.
pub fn page_schema() -> filter::Schema {
    use filter::Type::*;
    let mut s = filter::Schema::new();
    s.insert("title", Str);
    s.insert("url", Str);
    s.insert("path", Str);
    s.insert("dir", Str);
    s.insert("stem", Str);
    s.insert("layout", Str);
    s.insert("description", Str);
    s.insert("rendered", Bool);
    s.insert("toc", Bool);
    s.insert("order", Int);
    // §6f: the row's locale, always set (the default when no selector fired).
    s.insert("locale", Str);
    s
}

impl filter::Row for Page {
    fn field(&self, name: &str) -> filter::Value {
        use filter::Value as V;
        let opt = |o: &Option<String>| match o {
            Some(s) => V::Str(s.clone()),
            None => V::Null,
        };
        match name {
            "title" => opt(&self.title),
            "url" => V::Str(self.url.clone()),
            "path" => V::Str(self.rel.to_string_lossy().to_string()),
            "dir" => V::Str(
                self.rel
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default(),
            ),
            // §6f: the LOGICAL stem — `red-lentil-dal.fr.md` is still
            // `red-lentil-dal`, so `stem != "index"` means the same thing
            // in every locale.
            "stem" => Path::new(&self.logical)
                .file_stem()
                .map_or(V::Null, |s| V::Str(s.to_string_lossy().to_string())),
            "layout" => opt(&self.layout),
            "description" => opt(&self.description),
            "rendered" => V::Bool(self.rendered),
            "toc" => V::Bool(self.toc),
            "order" => self.order.map_or(V::Null, V::Int),
            "locale" => V::Str(self.locale.clone()),
            // Schema fields (§5b) resolve after the base names.
            other => self.fields.get(other).cloned().unwrap_or(V::Null),
        }
    }
}

/// Fields a filter may reference on an object row (§5 audit gap 1: objects
/// had no schema, so `over = "objects"` couldn't type-check a filter).
/// Dimensions are deliberately absent: they are render-time facts from the
/// thumbnail pass (q26), not load-time columns — a field that would need
/// every image decoded at load is not worth a filter yet.
pub fn object_schema() -> filter::Schema {
    use filter::Type::*;
    let mut s = filter::Schema::new();
    s.insert("path", Str);
    s.insert("dir", Str);
    s.insert("name", Str);
    s.insert("stem", Str);
    s.insert("ext", Str);
    s.insert("url", Str);
    s.insert("size", Int);
    s
}

impl filter::Row for Object {
    fn field(&self, name: &str) -> filter::Value {
        use filter::Value as V;
        match name {
            "path" => V::Str(self.rel.to_string_lossy().to_string()),
            "dir" => V::Str(
                self.rel
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default(),
            ),
            "name" => V::Str(self.name.clone()),
            "stem" => self
                .rel
                .file_stem()
                .map_or(V::Null, |s| V::Str(s.to_string_lossy().to_string())),
            "ext" => V::Str(self.ext.clone()),
            "url" => V::Str(self.url.clone()),
            "size" => V::Int(self.size as i64),
            _ => V::Null,
        }
    }
}

// ------------------------------------------------------------------ load

impl SiteDb {
    pub fn load(cfg: &Config) -> Result<Self> {
        let mut db = SiteDb::default();
        let t_m = std::time::Instant::now();
        let root = cfg.root();
        let markers = Markers::scan(&root, &cfg.markers, cfg.gitignore)?;
        db.stats.markers_ms = t_m.elapsed().as_secs_f64() * 1000.0;
        db.stats.markers = markers.found;

        // The engine-vocabulary walk: `.section` scope markers (§6e) and
        // `.schema.toml` field declarations (§5b) — positional names like
        // `.slots/`, no config entries. One name-only pass with the same
        // .gitignore defence as the marker scan.
        let mut b = store::walker(&root, cfg.gitignore);
        b.filter_entry(|e| !(e.file_type().is_some_and(|t| t.is_dir()) && e.file_name() == ".git"));
        for entry in b.build().filter_map(|e| e.ok()) {
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let Ok(rel) = entry.path().strip_prefix(&root) else { continue };
            let Some(dir) = rel.parent() else { continue };
            if entry.file_name() == ".section" {
                db.sections.push(dir.to_path_buf());
            } else if entry.file_name() == ".schema.toml" {
                let text = std::fs::read_to_string(entry.path())
                    .with_context(|| format!("reading {}", entry.path().display()))?;
                db.schemas.add(dir, &text, rel)?;
            }
        }
        db.sections.sort();
        let mut tree_c = None;
        let mut obj_c = None;

        for (name, c) in &cfg.collections {
            match c.kind {
                Kind::Posts => {
                    let (table, read_ms, index_ms) = build_posts(cfg, name, c, &markers)?;
                    db.posts = table;
                    db.stats.read_ms += read_ms;
                    db.stats.index_ms += index_ms;
                }
                Kind::Tree => tree_c = Some(c),
                Kind::Objects => obj_c = Some(c),
            }
        }

        let t = std::time::Instant::now();
        let (pages, objects) =
            build_tree_and_objects(cfg, tree_c, obj_c, &markers, &db.schemas)?;
        db.pages = pages;
        db.objects = objects;
        db.stats.read_ms += t.elapsed().as_secs_f64() * 1000.0;

        // Unified route list.
        let t = std::time::Instant::now();
        let route_locale = |l: &str| {
            (l != cfg.i18n.default).then(|| l.to_string())
        };
        for p in &db.posts.rows {
            db.routes.push(Route {
                source: Some(p.path.clone()),
                draft: p.draft,
                hidden: p.hidden,
                locale: route_locale(&p.locale),
                ..Route::new(p.url.clone(), RouteKind::Post)
            });
        }
        for p in &db.pages.rows {
            let kind = if p.rendered { RouteKind::Page } else { RouteKind::Static };
            db.routes.push(Route {
                source: Some(p.path.clone()),
                locale: route_locale(&p.locale),
                ..Route::new(p.url.clone(), kind)
            });
        }
        for o in &db.objects.rows {
            db.routes.push(Route {
                source: Some(o.path.clone()),
                ..Route::new(o.url.clone(), RouteKind::Object)
            });
        }
        crate::views::build_views(cfg, &mut db)?;
        crate::views::build_star_views(cfg, &mut db)?;
        db.stats.views_ms = t.elapsed().as_secs_f64() * 1000.0;

        // Constraint: route collisions across every table.
        let mut seen: HashMap<&str, &Route> = HashMap::new();
        let mut collisions = Vec::new();
        for r in &db.routes {
            if let Some(prev) = seen.insert(&r.url, r) {
                collisions.push(format!(
                    "  {}\n    {:?} {}\n    {:?} {}",
                    r.url,
                    prev.kind,
                    prev.source
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| format!("view {:?}", prev.view)),
                    r.kind,
                    r.source
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| format!("view {:?}", r.view)),
                ));
            }
        }
        if !collisions.is_empty() {
            bail!("route collisions:\n{}", collisions.join("\n"));
        }

        db.routes.sort_by(|a, b| a.url.cmp(&b.url));
        Ok(db)
    }
}

#[cfg(test)]
mod route_stem_tests {
    use super::*;
    use crate::filter::Filter;

    /// `stem != "index"` must drop index pages while letting sourceless
    /// (view) routes and dated posts through — Null passes `!=` by the
    /// filter's comparison rule, and that is load-bearing here.
    #[test]
    fn stem_filters_index_pages_only() {
        let f = Filter::parse("stem != \"index\"", &route_schema()).unwrap();
        let index_page = Route {
            source: Some(PathBuf::from("recipes/index.md")),
            ..Route::new("/recipes/".into(), RouteKind::Page)
        };
        let content_page = Route {
            source: Some(PathBuf::from("recipes/carbonara.md")),
            ..Route::new("/recipes/carbonara/".into(), RouteKind::Page)
        };
        let view_route = Route::new("/blog/".into(), RouteKind::View);
        assert!(!f.eval(&index_page));
        assert!(f.eval(&content_page));
        assert!(f.eval(&view_route));
    }
}

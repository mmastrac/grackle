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
pub struct Row {
    /// The collection whose source claimed this file. Relations anchor to
    /// it: adjacency over the whole posts TABLE interleaved two dated
    /// collections, so a blog post's "later post" could be a note (proved
    /// on a two-collection site before this existed).
    pub collection: String,
    pub path: PathBuf,
    pub rel: PathBuf,
    #[serde(serialize_with = "hex")]
    pub version: u64,
    pub date: Option<NaiveDate>,
    pub slug: String,
    /// Filename without extension — unique, because it carries the date.
    pub stem: String,
    /// `Option` because a PAGE may genuinely have none — a titleless page
    /// is searchable by body and wears its URL as the only honest label.
    /// A post's loader always fills it (front matter, else the slug read
    /// as words), so this is `Some` for every post; the option exists to
    /// let one row type serve both (q51).
    pub title: Option<String>,
    pub description: Option<String>,
    pub layout: Option<String>,
    pub tags: Vec<String>,
    /// Which theme renders this row, and how much wrapper it wears (§5a,
    /// §5g). `Page` has carried both since they existed; `Post` did not,
    /// so `FrontMatter` parsed `theme:`/`shell:` on a post and dropped
    /// them without a word. Step one of q51's merge is that both row types
    /// hold the same fields.
    pub theme: Option<String>,
    pub shell: Option<String>,
    /// Typed extra fields, validated against the governing `.schema.toml`
    /// (§5b) — the same mechanism pages have had. `.schema.toml` files were
    /// already collected by a ROOT-WIDE walk, so the declarations were
    /// visible to every table and only the tree loader consulted them;
    /// `read_posts` never called `validate` and never read
    /// `raw.front.extra`, so a post's extra keys parsed and evaporated.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, filter::Value>,
    /// The image-typed subset: field name -> root-relative source path.
    #[serde(skip)]
    pub images: BTreeMap<String, String>,
    /// Declared position (§6e), the last field `Page` had and `Post` did
    /// not. `FrontMatter` parsed `order:` either way and the posts loader
    /// dropped it, so the asymmetry was invisible rather than intentional.
    /// A post's *table* order is chronological; this is what a view sorts
    /// on when it says so — see `order_by` in `build_views`.
    pub order: Option<i64>,
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
    /// Tree heritage: a rendered row (front matter) vs a static file copied
    /// verbatim. Always true for a row that came from a posts collection —
    /// a post with no front matter is still parsed.
    pub rendered: bool,
    pub size: u64,
    /// q45: this row is a landing view's content — no standalone route,
    /// excluded from every query structurally.
    #[serde(skip)]
    pub claimed: bool,
}

impl Row {
    pub fn year_month(&self) -> Option<(i32, u32)> {
        use chrono::Datelike;
        self.date.map(|d| (d.year(), d.month()))
    }
}

impl Row {
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

/// Front matter's `date:`, for either table. `YYYY-MM-DD`; a bare
/// `YYYY-MM` means the first of that month, which is what the tree side
/// was spelling as a string field before it could hold a real date.
fn front_matter_date(raw: &str, path: &Path) -> Result<NaiveDate> {
    let s = raw.trim();
    let parsed = NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(&format!("{s}-01"), "%Y-%m-%d"));
    parsed.with_context(|| {
        format!(
            "{}: date: {s:?} is not YYYY-MM-DD (or YYYY-MM)",
            path.display()
        )
    })
}

/// `2022-03-16` — sortable; the machine-readable date everywhere.
pub fn iso_date(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// `16 March 2022` — what themes and search hits show.
pub fn pretty_date(d: NaiveDate) -> String {
    d.format("%-d %B %Y").to_string()
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
    pub rows: Vec<Row>,
    /// The primary index (DESIGN.md §3): `(date, slug)`, unique.
    /// NOT `slug` alone — measured: `not-dead-yet` is used by both a 2003 and
    /// a 2006 post, which is legal because their dates (and so URLs) differ.
    #[serde(skip)]
    pub by_key: HashMap<(Option<NaiveDate>, String), usize>,
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

#[cfg(test)]
mod adjacency_tests {
    use super::*;

    /// `neighbors_in` is now just a walk — the reach was decided when the
    /// sequence was built (see `views::build_adjacency`, and the tests
    /// there for what the sequence contains).
    #[test]
    fn walks_the_sequence_in_both_directions() {
        let seq = [3, 2, 1, 0];
        let (newer, older) = PostsTable::neighbors_in(&seq, 2);
        assert_eq!((newer, older), (Some(3), Some(1)));

        // The ends terminate rather than wrap.
        assert_eq!(PostsTable::neighbors_in(&seq, 3), (None, Some(2)));
        assert_eq!(PostsTable::neighbors_in(&seq, 0), (Some(1), None));

        // A row absent from the sequence has no neighbours at all — which
        // is how a filtered-out row (a draft, under a declared set) stops
        // appearing as someone's later post.
        assert_eq!(PostsTable::neighbors_in(&seq, 9), (None, None));
    }
}

impl PostsTable {
    /// Walk a prepared sequence. The sequence IS the reach (q51), so there
    /// is no collection filter here any more: `db.adjacency` is built per
    /// collection, and a declared `adjacency` set carries its own `from`.
    ///
    /// The bug this replaced: `order` spanned every collection feeding the
    /// table, so walking it raw made a blog post's neighbour a note
    /// whenever two dated collections existed — measured on a
    /// two-collection site, the January blog post linked February's and
    /// April's *notes*. `_posts` and `_drafts` never showed it because
    /// drafts are undated and so absent from `order` entirely, which is
    /// exactly the accident a declared set replaces with a rule.
    pub fn neighbors_in(seq: &[usize], idx: usize) -> (Option<usize>, Option<usize>) {
        let Some(pos) = seq.iter().position(|&i| i == idx) else {
            return (None, None);
        };
        (
            seq[..pos].iter().next_back().copied(),
            seq.get(pos + 1).copied(),
        )
    }
}

#[derive(Debug, Default, Serialize)]
pub struct TreeTable {
    pub rows: Vec<Row>,
    /// §6f: logical identity -> every locale variant, rendered rows only.
    #[serde(skip)]
    pub by_logical: HashMap<String, Vec<usize>>,
    /// URL -> row. The posts table has always had one; the tree did not, so
    /// three sites linear-scanned every page to answer "which row is this
    /// route?" (q51's census).
    #[serde(skip)]
    pub by_url: HashMap<String, usize>,
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
    /// The sequence `next`/`previous` step through, per posts collection
    /// (q51). Built from the collection's declared `adjacency` set, or —
    /// unset — every row of the collection in the default locale, newest
    /// first, which is what `PostsTable::order` used to supply implicitly.
    #[serde(skip)]
    pub adjacency: BTreeMap<String, Vec<usize>>,
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

/// Read one posts collection's rows. Indexing is deliberately NOT here:
/// several collections can contribute to the one posts table (`_posts` and
/// `_drafts`), and an index built per collection would see only part of the
/// corpus — `by_url` could not detect a collision between them, and `order`
/// would restart per source.
fn read_posts(
    cfg: &Config,
    name: &str,
    c: &Collection,
    markers: &Markers,
    schemas: &crate::schema::Schemas,
) -> Result<(Vec<Row>, f64)> {
    // Bound here because the row loop shadows `name` with the post's own
    // path identity — silently, since both are strings.
    let collection = name.to_string();
    let root = cfg.root();
    let source_rel = PathBuf::from(
        c.source
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("collection {name} has kind=posts but no source"))?,
    );
    let source = root.join(&source_rel);

    let t0 = std::time::Instant::now();
    let raws: Vec<RawRow> = store::load_dir(&source, &["md", "markdown"])?;
    let read_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let formats: Vec<FilenameFormat> = c
        .filename_formats
        .iter()
        .map(|f| FilenameFormat::compile(f))
        .collect::<Result<_>>()?;
    if formats.is_empty() {
        bail!("collection {name} has kind=posts but no filename_formats");
    }
    let rules = compile_rules(c)?;

    let mut rows: Vec<Row> = Vec::with_capacity(raws.len());
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

        // `logical` keeps its extension, matching the tree side — where the
        // convention is config-visible (`content = "recipes/index.md"`).
        // The two loaders used OPPOSITE conventions for this field, and
        // `Page::field` only derived `stem` correctly because of it.
        let logical = logical_rel.to_string_lossy().to_string();
        let key = formats.iter().find_map(|f| f.parse(&stem));
        let from_name = match &key {
            Some(k) => Some(
                NaiveDate::from_ymd_opt(k.year, k.month, k.day).with_context(|| {
                    format!(
                        "{} has an impossible date in its filename",
                        raw.path.display()
                    )
                })?,
            ),
            None => None,
        };
        // Front matter beats the filename, the same precedence every other
        // field has (§4b) — and the same `date:` a tree page now carries.
        // Before this it landed in `extra`, where a governed post rejected
        // it as undeclared and an ungoverned one dropped it.
        let date = match &raw.front.date {
            Some(s) => Some(front_matter_date(s, &raw.path)?),
            None => from_name,
        };
        let slug = key
            .as_ref()
            .map(|k| k.slug.clone())
            .unwrap_or_else(|| stem.clone());

        let (route_tmpl, rule_defaults) = apply_rules(&rules, &logical_rel, true);
        // Precedence (§4b): front matter > nearest marker > rule. Markers are
        // inserted first so `or_insert` cannot let a rule override them.
        let root_rel = raw
            .path
            .strip_prefix(&root)
            .unwrap_or(&raw.rel)
            .to_path_buf();
        let mut defaults: BTreeMap<&str, &toml::Value> = BTreeMap::new();
        let marker_defaults = markers.defaults_for(&root_rel);
        for (k, v) in &marker_defaults {
            defaults.insert(k.as_str(), v);
        }
        for (k, v) in rule_defaults {
            defaults.entry(k).or_insert(v);
        }
        let title = Some(
            raw.front
                .title
                .clone()
                .unwrap_or_else(|| slug.replace('-', " ")),
        );
        // Governance follows the LOGICAL path (§6f), exactly as the tree
        // loader does it: a translation is governed by its original's
        // `.schema.toml`.
        // `raw.rel` is relative to the collection SOURCE, while schemas are
        // keyed root-relative by the root-wide `.schema.toml` walk — so a
        // `_posts/.schema.toml` is registered under `_posts` and resolving
        // the bare filename would never find it.
        let parent = source_rel
            .join(&raw.rel)
            .parent()
            .unwrap_or(Path::new(""))
            .to_path_buf();
        let checked = match schemas.resolve(&parent) {
            Some(schema) => crate::schema::validate(&schema, &raw.front.extra, &raw.path)?,
            None => Default::default(),
        };
        let theme = raw.front.theme.clone().or_else(|| {
            defaults
                .get("theme")
                .and_then(|v| v.as_str())
                .map(String::from)
        });
        let shell = raw.front.shell.clone().or_else(|| {
            defaults
                .get("shell")
                .and_then(|v| v.as_str())
                .map(String::from)
        });
        let draft = raw
            .front
            .draft
            .unwrap_or_else(|| as_bool(&defaults, "draft"));
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
        let url = if locale != cfg.i18n.default {
            format!("/{locale}{url}")
        } else {
            url
        };

        rows.push(Row {
            collection: collection.clone(),
            path: raw.path,
            // ROOT-relative since the merge, so `path`/`dir` mean one thing
            // on either table. Rule globs still match the collection-
            // relative form (`apply_rules` takes `logical_rel`), which is
            // what `match = "hidden/**"` inside `_posts` has always meant.
            rel: source_rel.join(&raw.rel),
            version: raw.version,
            date,
            slug,
            stem,
            title,
            description: raw.front.description,
            layout,
            tags: raw.front.tags,
            theme,
            shell,
            fields: checked.values,
            images: checked.images,
            order: raw.front.order,
            draft,
            hidden,
            noindex,
            toc,
            locale,
            logical,
            url,
            body_bytes: raw.body.len(),
            body: raw.body,
            // A post is always parsed; the tree distinction does not apply.
            rendered: true,
            size: 0,
            claimed: false,
        });
    }

    Ok((rows, read_ms))
}

/// Index the whole posts table at once, over every collection's rows.
fn index_posts(cfg: &Config, mut rows: Vec<Row>) -> Result<PostsTable> {
    rows.sort_by(|a, b| a.path.cmp(&b.path));

    // The reverse-chronological index used to be built here, and carried
    // three things at once: the sort, undated-last, and a DEFAULT-LOCALE
    // filter that quietly made every listing, feed and archive
    // single-locale. All three are now stated where they are used —
    // `views::chronological` plus an explicit locale filter — so the table
    // holds identity indexes only and the merge has nothing to inherit.
    let mut table = PostsTable::default();

    let mut seen_names: HashMap<String, usize> = HashMap::new();
    for (i, p) in rows.iter().enumerate() {
        // Identity indexes span all locales: URLs are globally unique. The
        // `name` uniqueness guard that used to sit here is retired with the
        // field — it existed because `name` dropped the extension, so
        // `foo.md` and `foo.markdown` collided. A root-relative `rel` keeps
        // them distinct by construction.
        if let Some(prev) = table.by_url.insert(p.url.clone(), i) {
            bail!(
                "route collision at {}:\n  {}\n  {}",
                p.url,
                rows[prev].path.display(),
                p.path.display()
            );
        }
        table
            .by_logical
            .entry(p.logical.clone())
            .or_default()
            .push(i);
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
    Ok(table)
}

// ------------------------------------------------------- tree + objects

/// One walk of the site root, partitioned by membership precedence
/// (DESIGN.md §3): objects win by extension, tree takes the rest.
fn build_tree_and_objects(
    cfg: &Config,
    tree_name: &str,
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
        // Nor is the config that declared all of this. Matched by identity,
        // not by glob, so a site needs no `exclude` entry to avoid
        // publishing its own grackle.toml.
        .filter(|f| {
            std::fs::canonicalize(&f.path)
                .map(|p| p != cfg.config_file)
                .unwrap_or(true)
        })
        .collect();

    // q45: rows named by a view's `content` — claimed landings. Matched
    // by logical identity so every locale variant is claimed with its
    // original.
    let claims = cfg.content_claims();

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
        let url = if locale != cfg.i18n.default {
            format!("/{locale}{url}")
        } else {
            url
        };

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
            let shell = fm.shell.clone().or_else(|| {
                defaults
                    .get("shell")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            });
            // A typo'd shell would silently render the wrong tier — the
            // failure mode this codebase keeps finding. Closed vocabulary,
            // checked at load.
            if let Some(sh) = shell.as_deref() {
                if !matches!(sh, "none" | "light" | "html") {
                    anyhow::bail!(
                        "{}: shell = \"{sh}\" is not a shell — expected none, light or html (§5g)",
                        f.rel.display()
                    );
                }
            }
            let draft = fm.draft.unwrap_or_else(|| as_bool(&defaults, "draft"));
            let hidden = fm.hidden.unwrap_or_else(|| as_bool(&defaults, "hidden"));
            let noindex = fm.noindex.unwrap_or_else(|| as_bool(&defaults, "noindex"));
            let date = match &fm.date {
                Some(s) => Some(front_matter_date(s, &f.path)?),
                None => None,
            };
            let logical = logical_rel.to_string_lossy().to_string();
            if f.has_front_matter {
                pages
                    .by_logical
                    .entry(logical.clone())
                    .or_default()
                    .push(pages.rows.len());
            }
            // q45: a row named by some view's `content` is claimed — every
            // locale variant of it (the claim is on the logical identity).
            let claimed = claims.contains_key(logical.as_str());
            if claimed && !f.has_front_matter {
                bail!(
                    "view {}: content {logical:?} has no front matter, so it \
                     is a static file, not a claimable row",
                    claims[logical.as_str()]
                );
            }
            // `stem` is STORED, not derived. `Page::field` used to recompute
            // it from `logical` via `file_stem()`, which was correct only
            // because the tree kept the extension that the posts loader
            // stripped — a page named `v1.2-release.md` would have come back
            // `v1` the moment those conventions were unified. Computed once
            // here from the real path, the question stops existing.
            let stem = logical_rel
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            pages.rows.push(Row {
                collection: tree_name.to_string(),
                slug: stem.clone(),
                stem,
                // The tree loader does not hold bodies: pages are re-read at
                // render time (§2). That asymmetry is loader-shaped, not row-
                // shaped, and outlives the merge.
                body: String::new(),
                body_bytes: 0,
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
                date,
                tags: fm.tags,
                toc: fm.toc.unwrap_or(false),
                theme,
                shell,
                draft,
                hidden,
                noindex,
                fields: checked.values,
                images: checked.images,
                locale,
                logical,
                claimed,
            });
        }
    }
    // Every claim must have found its row — a typo'd content path is a
    // load error naming the view, not a silently bare landing.
    for (path, view) in &claims {
        if !pages.rows.iter().any(|p| p.claimed && p.logical == *path) {
            bail!("view {view}: content {path:?} names no row in the tree");
        }
    }
    for (i, p) in pages.rows.iter().enumerate() {
        if !p.url.is_empty() {
            pages.by_url.insert(p.url.clone(), i);
        }
    }
    Ok((pages, objects))
}

/// Front matter of a tree page: presentation reads its fields directly.
/// A parse failure is a LOAD ERROR naming the file — this used to swallow
/// bad YAML into an empty schema, and an unquoted `title: A: B` shipped a
/// silently titleless page. Loud beats lenient (§4's constraint ethos).
fn read_page_schema(path: &Path) -> Result<crate::store::FrontMatter> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let (yaml, _) = store::split_front_matter(&text);
    serde_yaml_ng::from_str(yaml).with_context(|| format!("front matter of {}", path.display()))
}

// ------------------------------------------------------------------ views

/// Fields a filter may reference on a row, and their types. Everything else
/// is a load-time error (filter.rs), so a typo can't silently match
/// everything. Was two functions, `post_schema` and `page_schema`, differing
/// by five names — the union is additive both ways (q51).
pub fn row_schema() -> filter::Schema {
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
    s.insert("order", Int);
    // A post has carried `toc` as long as pages have, cascading from
    // markers and rules and driving the render — but `post_schema` never
    // declared it and `Post::field` never answered, so no query could name
    // it while a page's could. Found by diffing the two schemas rather than
    // by reading the field census, which is how it survived four slices
    // aimed squarely at this class of bug.
    s.insert("toc", Bool);
    s.insert("tags", List);
    // Was page-only. `rendered` is true for every post, and `path`/`dir`
    // read a `rel` that means one thing on either table since the merge.
    s.insert("rendered", Bool);
    s.insert("path", Str);
    s.insert("dir", Str);
    // §6f: the row's locale, always set (the default when no selector fired).
    s.insert("locale", Str);
    s
}

impl filter::Row for Row {
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
            "title" => opt_str(&self.title),
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
            "order" => self.order.map_or(V::Null, V::Int),
            "toc" => V::Bool(self.toc),
            "rendered" => V::Bool(self.rendered),
            // `rel` is root-relative for every row since the merge, so
            // these mean one thing whichever table the row came from.
            "path" => V::Str(self.rel.to_string_lossy().to_string()),
            "dir" => V::Str(
                self.rel
                    .parent()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default(),
            ),
            "tags" => V::List(self.tags.clone()),
            "locale" => V::Str(self.locale.clone()),
            // Schema fields (§5b) resolve after the base names — the same
            // fallthrough a page has had.
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
    /// The row a route points at, whichever table holds it. Row identity is
    /// one thing since q51; only the storage is still two.
    pub fn row_by_url(&self, url: &str) -> Option<&Row> {
        self.posts
            .by_url
            .get(url)
            .map(|&i| &self.posts.rows[i])
            .or_else(|| self.pages.by_url.get(url).map(|&i| &self.pages.rows[i]))
    }

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
            let Ok(rel) = entry.path().strip_prefix(&root) else {
                continue;
            };
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

        // Several collections may feed the posts table — `_posts` and
        // `_drafts` are two sources of one corpus — so rows are gathered
        // first and indexed once, over all of them.
        let mut post_rows: Vec<Row> = Vec::new();
        let mut tree_name = String::new();
        for (name, c) in &cfg.collections {
            match c.kind {
                Kind::Posts => {
                    let (rows, read_ms) = read_posts(cfg, name, c, &markers, &db.schemas)?;
                    post_rows.extend(rows);
                    db.stats.read_ms += read_ms;
                }
                Kind::Tree => {
                    tree_c = Some(c);
                    tree_name = name.clone();
                }
                Kind::Objects => obj_c = Some(c),
            }
        }
        let t_index = std::time::Instant::now();
        db.posts = index_posts(cfg, post_rows)?;
        db.stats.index_ms += t_index.elapsed().as_secs_f64() * 1000.0;

        let t = std::time::Instant::now();
        let (pages, objects) =
            build_tree_and_objects(cfg, &tree_name, tree_c, obj_c, &markers, &db.schemas)?;
        db.pages = pages;
        db.objects = objects;
        db.stats.read_ms += t.elapsed().as_secs_f64() * 1000.0;

        // Unified route list.
        let t = std::time::Instant::now();
        let route_locale = |l: &str| (l != cfg.i18n.default).then(|| l.to_string());
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
            // q45: a claimed row has no route of its own — the owning
            // view materializes the landing.
            if p.claimed {
                continue;
            }
            let kind = if p.rendered {
                RouteKind::Page
            } else {
                RouteKind::Static
            };
            db.routes.push(Route {
                source: Some(p.path.clone()),
                locale: route_locale(&p.locale),
                draft: p.draft,
                hidden: p.hidden,
                ..Route::new(p.url.clone(), kind)
            });
        }
        for o in &db.objects.rows {
            db.routes.push(Route {
                source: Some(o.path.clone()),
                ..Route::new(o.url.clone(), RouteKind::Object)
            });
        }
        crate::views::build_adjacency(cfg, &mut db)?;
        crate::views::build_views(cfg, &mut db)?;
        crate::views::build_star_views(cfg, &mut db)?;
        db.stats.views_ms = t.elapsed().as_secs_f64() * 1000.0;

        // q45: a claimed row's URL becomes its landing's — the owning
        // view's route in the row's locale — so source-path links and the
        // ancestors walk see the landing, not the retired standalone URL.
        // A locale variant whose partition didn't materialize keeps no
        // URL (nothing may link it).
        {
            let claims = cfg.content_claims();
            let mut fixed: Vec<(usize, String)> = Vec::new();
            for (i, p) in db.pages.rows.iter().enumerate() {
                if !p.claimed {
                    continue;
                }
                let owner = claims[p.logical.as_str()];
                let url = db
                    .routes
                    .iter()
                    .find(|r| {
                        r.kind == RouteKind::View
                            && r.view.as_deref() == Some(owner)
                            && r.locale == route_locale(&p.locale)
                            && r.key.is_none()
                            && r.page.is_none_or(|n| n == 1)
                    })
                    .map(|r| r.url.clone());
                fixed.push((i, url.unwrap_or_default()));
            }
            for (i, url) in fixed {
                db.pages.rows[i].url = url;
            }
        }

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

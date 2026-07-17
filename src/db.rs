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
    s
}

impl filter::Row for Route {
    fn field(&self, name: &str) -> filter::Value {
        use filter::Value as V;
        match name {
            "kind" => V::Str(format!("{:?}", self.kind).to_lowercase()),
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
    pub stats: LoadStats,
}

/// A routeless view's resolved rows.
#[derive(Debug, Default, Serialize)]
pub struct ViewRows {
    /// None means query-only: a named set, not something renderable.
    pub layout: Option<String>,
    pub rows: usize,
    #[serde(skip)]
    pub members: Vec<usize>,
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
        let stem: String = raw
            .path
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

        let (route_tmpl, rule_defaults) = apply_rules(&rules, &raw.rel, true);
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
            url,
            body_bytes: raw.body.len(),
            body: raw.body,
        });
    }

    rows.sort_by(|a, b| a.path.cmp(&b.path));

    let mut order: Vec<usize> = (0..rows.len()).collect();
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
        if let Some(prev) = table.by_key.insert((p.date, p.slug.clone()), i) {
            bail!(
                "duplicate (date, slug) key ({}, {:?}):\n  {}\n  {}",
                p.date.map(|d| d.to_string()).unwrap_or("none".into()),
                p.slug,
                rows[prev].path.display(),
                p.path.display()
            );
        }
        if let Some(prev) = table.by_name.insert(p.name.clone(), i) {
            bail!(
                "duplicate post name {:?} ({{% post_url %}} would be ambiguous):\n  {}\n  {}",
                p.name,
                rows[prev].path.display(),
                p.path.display()
            );
        }
        table.by_slug.entry(p.slug.clone()).or_default().push(i);
        if let Some(prev) = table.by_url.insert(p.url.clone(), i) {
            bail!(
                "route collision at {}:\n  {}\n  {}",
                p.url,
                rows[prev].path.display(),
                p.path.display()
            );
        }
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

        let rules = if is_object { &obj_rules } else { &tree_rules };
        let (tmpl, _defaults) = apply_rules(rules, &f.rel, f.has_front_matter);
        let Some(tmpl) = tmpl else {
            bail!("no rule supplies a route for {}", f.path.display());
        };
        let url = tidy(
            route::render(tmpl, |k| path_tokens(&f.rel, k))
                .with_context(|| format!("routing {}", f.path.display()))?,
        );

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
            let (title, layout) = if f.has_front_matter {
                read_page_schema(&f.path)
            } else {
                (None, None)
            };
            pages.rows.push(Page {
                path: f.path,
                rel: f.rel,
                version: f.version,
                url,
                rendered: f.has_front_matter,
                size: f.size,
                title,
                layout,
            });
        }
    }
    Ok((pages, objects))
}

/// Front matter of a tree page: just the fields presentation needs.
fn read_page_schema(path: &Path) -> (Option<String>, Option<String>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return (None, None);
    };
    let Some(rest) = text.strip_prefix("---") else {
        return (None, None);
    };
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    let end = rest.find("\n---").unwrap_or(rest.len());
    let fm: crate::store::FrontMatter =
        serde_yaml_ng::from_str(&rest[..end]).unwrap_or_default();
    (fm.title, fm.layout)
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
            _ => V::Null,
        }
    }
}

/// One group key a row contributes under a single `group_by` spec: the typed
/// sort component (years/months order numerically, tags lexically), the
/// display component (joined into `Route.key`), and the parameters the key
/// exposes to route/`title`/`crumb` templates.
#[derive(Clone, Debug)]
struct GroupKey {
    sort: SortKey,
    params: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SortKey {
    Int(i64),
    Str(String),
}

impl SortKey {
    fn display(&self) -> String {
        match self {
            // Numeric parts zero-pad to two so `2022-3` reads `2022-03` —
            // years are 4 digits and unaffected.
            SortKey::Int(n) if *n < 10 => format!("0{n}"),
            SortKey::Int(n) => n.to_string(),
            SortKey::Str(s) => s.clone(),
        }
    }
}

/// The group keys a row holds under one spec. Empty means the row is absent
/// from this partition (an undated row under a date grouping).
fn group_keys(p: &Post, spec: &str) -> Result<Vec<GroupKey>> {
    use chrono::Datelike;
    Ok(match spec {
        "tags" => p
            .tags
            .iter()
            .map(|t| GroupKey {
                sort: SortKey::Str(t.clone()),
                params: vec![("key".into(), t.clone())],
            })
            .collect(),
        "date.year" => p
            .date
            .into_iter()
            .map(|d| GroupKey {
                sort: SortKey::Int(d.year() as i64),
                params: vec![("year".into(), d.year().to_string())],
            })
            .collect(),
        "date.month" => p
            .date
            .into_iter()
            .map(|d| GroupKey {
                sort: SortKey::Int(d.month() as i64),
                params: vec![
                    ("month".into(), d.month().to_string()),
                    ("month_name".into(), d.format("%B").to_string()),
                ],
            })
            .collect(),
        other => bail!("unsupported group_by {other:?} (have: tags, date.year, date.month)"),
    })
}

/// The composite keys a row belongs to under a subdivision chain — the
/// cartesian product across levels (`tags` can multi-key a row; date specs
/// contribute at most one each). Empty when the row is absent at any level.
fn key_combos(p: &Post, chain: &[String]) -> Result<Vec<Vec<GroupKey>>> {
    let mut combos: Vec<Vec<GroupKey>> = vec![Vec::new()];
    for spec in chain {
        let keys = group_keys(p, spec)?;
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        combos = combos
            .into_iter()
            .flat_map(|c| {
                keys.iter().map(move |k| {
                    let mut c2 = c.clone();
                    c2.push(k.clone());
                    c2
                })
            })
            .collect();
    }
    Ok(combos)
}

/// The `group_by` specs governing a view, outermost ancestor first. This is
/// subdivision (§5c): a grouped view `over` a grouped view refines the
/// parent's partition, so the parent's spec applies before the child's. Read
/// from config alone — no dependency on view processing order. The chain is
/// acyclic and the composition shape is legal because `Config::query` already
/// validated both.
fn group_chain(cfg: &Config, name: &str) -> Vec<String> {
    let mut chain = Vec::new();
    let mut cur = name;
    while let Some(v) = cfg.views.get(cur) {
        if let Some(g) = &v.group_by {
            chain.push(g.clone());
        }
        cur = &v.over;
    }
    chain.reverse();
    chain
}

fn build_views(cfg: &Config, db: &mut SiteDb) -> Result<()> {
    for (name, v) in &cfg.views {
        // `over = "*"` views read the finished route set, so they run in a
        // second pass (see build_star_views). Views iterate in name order, so
        // running them inline made `sitemap` miss `tag_index` — 1544 not 1559.
        if v.over == "*" {
            continue;
        }
        // Both named queries (`published`) and embedded views (`latest`) still
        // have to resolve, so a typo in `over` is a startup error either way.
        let q = cfg.query(name)?;
        if q.base != "blog" {
            continue; // phase 1: posts-backed views only
        }
        // Parsed and type-checked once per view, not per row: a bad filter is a
        // startup error naming the view.
        let pred = match q.predicate() {
            Some(src) => filter::Filter::parse(&src, &post_schema())
                .with_context(|| format!("view {name}: filter {src:?}"))?,
            None => filter::Filter::always(),
        };
        let posts = &db.posts;
        let visible: Vec<usize> = posts
            .order
            .iter()
            .copied()
            .filter(|&i| pred.eval(&posts.rows[i]))
            .collect();

        // No route: one row set, and nowhere to hang it but the view itself.
        if !v.is_materialized() {
            let members: Vec<usize> =
                visible.into_iter().take(v.limit.unwrap_or(usize::MAX)).collect();
            db.views.insert(
                name.clone(),
                ViewRows {
                    layout: v.layout.clone(),
                    rows: members.len(),
                    members,
                },
            );
            continue;
        }

        // Grouped views, possibly a subdivision chain (§5c): a grouped view
        // `over` a grouped view refines the parent's partition, and the group
        // keys accumulate — GROUP BY year, month, expressed compositionally.
        // The chain is read from config alone, so processing order between
        // parent and child views doesn't matter.
        if v.group_by.is_some() {
            let tmpl = v
                .route
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("view {name} needs a route"))?;
            let chain = group_chain(cfg, name);
            // Composite key → (template params, members). BTreeMap on the
            // typed sort key keeps years/months numeric and tags lexical.
            let mut groups: BTreeMap<Vec<SortKey>, (Vec<(String, String)>, Vec<usize>)> =
                BTreeMap::new();
            for &i in &visible {
                for combo in key_combos(&posts.rows[i], &chain)? {
                    let sort: Vec<SortKey> = combo.iter().map(|k| k.sort.clone()).collect();
                    groups
                        .entry(sort)
                        .or_insert_with(|| {
                            let params =
                                combo.iter().flat_map(|k| k.params.clone()).collect();
                            (params, Vec::new())
                        })
                        .1
                        .push(i);
                }
            }
            for (sort, (params, members)) in groups {
                let url = route::render(tmpl, |k| {
                    params.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
                })?;
                let key = sort
                    .iter()
                    .map(SortKey::display)
                    .collect::<Vec<_>>()
                    .join("-");
                db.routes.push(Route {
                    url,
                    kind: RouteKind::View,
                    source: None,
                    view: Some(name.clone()),
                    key: Some(key),
                    rows: Some(members.len()),
                    page: None,
                    params,
                    draft: false,
                    hidden: false,
                    members,
                });
            }
            continue;
        }

        match v.group_by.as_deref() {
            // Paginated list.
            None if v.paginate.is_some() => {
                let per = v.paginate.unwrap().max(1);
                let pages = visible.len().div_ceil(per);
                for n in 1..=pages {
                    let tmpl = if n == 1 {
                        v.routes.first()
                    } else {
                        v.routes.get(1).or_else(|| v.routes.first())
                    };
                    let Some(tmpl) = tmpl else { continue };
                    let url = route::render(tmpl, |k| match k {
                        "n" => Some(n.to_string()),
                        _ => None,
                    })?;
                    let members: Vec<usize> =
                        visible.iter().copied().skip(per * (n - 1)).take(per).collect();
                    db.routes.push(Route {
                        url,
                        kind: RouteKind::View,
                        source: None,
                        view: Some(name.clone()),
                        key: Some(format!("page {n}")),
                        rows: Some(members.len()),
                        page: Some(n),
                        params: Vec::new(),
                        draft: false,
                        hidden: false,
                        members,
                    });
                }
            }
            // Single route over a (possibly limited) slice: the feed.
            None => {
                let tmpl = v
                    .route
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("view {name} needs a route"))?;
                let members: Vec<usize> = visible
                    .iter()
                    .copied()
                    .take(v.limit.unwrap_or(visible.len()))
                    .collect();
                db.routes.push(Route {
                    url: tmpl.to_string(),
                    kind: RouteKind::View,
                    source: None,
                    view: Some(name.clone()),
                    key: None,
                    rows: Some(members.len()),
                    page: None,
                    params: Vec::new(),
                    draft: false,
                    hidden: false,
                    members,
                });
            }
            Some(_) => unreachable!("grouped views are handled above"),
        }
    }
    Ok(())
}

/// Views over the whole route set (the sitemap). Runs after every other route
/// exists, and its `rows` is the count that actually passes its filter.
fn build_star_views(cfg: &Config, db: &mut SiteDb) -> Result<()> {
    for (name, v) in &cfg.views {
        if v.over != "*" {
            continue;
        }
        let tmpl = v
            .route
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("view {name} needs a route"))?;
        let pred = match &v.filter {
            Some(src) => filter::Filter::parse(src, &route_schema())
                .with_context(|| format!("view {name}: filter {src:?}"))?,
            None => filter::Filter::always(),
        };
        let rows = db.routes.iter().filter(|r| pred.eval(*r)).count();
        db.routes.push(Route {
            url: tmpl.to_string(),
            kind: RouteKind::View,
            source: None,
            view: Some(name.clone()),
            key: None,
            rows: Some(rows),
            page: None,
            params: Vec::new(),
            draft: false,
            hidden: false,
            members: Vec::new(),
        });
    }
    Ok(())
}

// ------------------------------------------------------------------ load

impl SiteDb {
    pub fn load(cfg: &Config) -> Result<Self> {
        let mut db = SiteDb::default();
        let t_m = std::time::Instant::now();
        let markers = Markers::scan(&cfg.root(), &cfg.markers, cfg.gitignore)?;
        db.stats.markers_ms = t_m.elapsed().as_secs_f64() * 1000.0;
        db.stats.markers = markers.found;
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
        let (pages, objects) = build_tree_and_objects(cfg, tree_c, obj_c, &markers)?;
        db.pages = pages;
        db.objects = objects;
        db.stats.read_ms += t.elapsed().as_secs_f64() * 1000.0;

        // Unified route list.
        let t = std::time::Instant::now();
        for p in &db.posts.rows {
            db.routes.push(Route {
                url: p.url.clone(),
                kind: RouteKind::Post,
                source: Some(p.path.clone()),
                view: None,
                key: None,
                rows: None,
                page: None,
                params: Vec::new(),
                draft: p.draft,
                hidden: p.hidden,
                members: Vec::new(),
            });
        }
        for p in &db.pages.rows {
            db.routes.push(Route {
                url: p.url.clone(),
                kind: if p.rendered {
                    RouteKind::Page
                } else {
                    RouteKind::Static
                },
                source: Some(p.path.clone()),
                view: None,
                key: None,
                rows: None,
            page: None,
            params: Vec::new(),
                draft: false,
                hidden: false,
                members: Vec::new(),
            });
        }
        for o in &db.objects.rows {
            db.routes.push(Route {
                url: o.url.clone(),
                kind: RouteKind::Object,
                source: Some(o.path.clone()),
                view: None,
                key: None,
                rows: None,
            page: None,
            params: Vec::new(),
                draft: false,
                hidden: false,
                members: Vec::new(),
            });
        }
        build_views(cfg, &mut db)?;
        build_star_views(cfg, &mut db)?;
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
mod grouping_tests {
    use super::*;

    fn post(date: Option<&str>, tags: &[&str]) -> Post {
        Post {
            date: date.map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap()),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            ..Post::default()
        }
    }

    #[test]
    fn subdivision_chain_accumulates_params() {
        let p = post(Some("2022-03-16"), &[]);
        let chain = vec!["date.year".to_string(), "date.month".to_string()];
        let combos = key_combos(&p, &chain).unwrap();
        assert_eq!(combos.len(), 1);
        let params: Vec<(String, String)> =
            combos[0].iter().flat_map(|k| k.params.clone()).collect();
        assert!(params.contains(&("year".into(), "2022".into())), "{params:?}");
        assert!(params.contains(&("month".into(), "3".into())), "{params:?}");
        assert!(params.contains(&("month_name".into(), "March".into())), "{params:?}");
        // Composite display joins with zero-padded numerics: "2022-03".
        let key: Vec<String> = combos[0].iter().map(|k| k.sort.display()).collect();
        assert_eq!(key.join("-"), "2022-03");
    }

    #[test]
    fn undated_rows_are_absent_from_date_partitions() {
        let p = post(None, &["rust"]);
        assert!(key_combos(&p, &["date.year".into()]).unwrap().is_empty());
        // ...but present in the tag partition.
        assert_eq!(key_combos(&p, &["tags".into()]).unwrap().len(), 1);
    }

    #[test]
    fn tags_multi_key_a_row() {
        let p = post(Some("2022-03-16"), &["c", "rust"]);
        let combos = key_combos(&p, &["tags".into()]).unwrap();
        assert_eq!(combos.len(), 2);
    }

    #[test]
    fn months_sort_numerically_not_lexically() {
        assert!(SortKey::Int(3) < SortKey::Int(12));
        assert_eq!(SortKey::Int(3).display(), "03");
        assert_eq!(SortKey::Int(2022).display(), "2022");
    }
}

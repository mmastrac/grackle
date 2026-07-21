//! The site's data model: what a row, an object and a route are, the fields a
//! filter may name on each, and the indexes and constraints over them.
//!
//! Everything domain-specific about grack.com lives here. The machinery it is
//! built from — the filter language, the index shapes — is `grackle-db`, which
//! knows none of it. Rows arrive through `SiteDb::insert_rows`; the layer that
//! produces them is `grackle-source`.

use grackle_db::{filter, Table};

use anyhow::Result;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

/// Which table a collection feeds. Table identity is model vocabulary, so it
/// lives here and config deserializes into it — that is what lets `ViewRows`
/// name a table without the loader owning the name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Posts,
    Tree,
    Objects,
}

/// The row field a group-by spec names. `date.year` is a spelling of the
/// `year` column, not a field of its own; everything else is itself.
///
/// Beside `row_schema` because that is what it resolves against — a spec is
/// only meaningful as a column name.
pub fn spec_field(spec: &str) -> &str {
    match spec {
        "date.year" => "year",
        "date.month" => "month",
        s => s,
    }
}

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
    /// Measured at load, when the body is briefly in hand. The body itself
    /// is not kept — every consumer re-reads it (§2).
    pub body_bytes: usize,
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

/// `2022-03-16` — sortable; the machine-readable date everywhere.
pub fn iso_date(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// `16 March 2022` — what themes and search hits show.
pub fn pretty_date(d: NaiveDate) -> String {
    d.format("%-d %B %Y").to_string()
}

fn hex<S: serde::Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&format!("{v:016x}"))
}

// ------------------------------------------------------------------ tables

#[cfg(test)]
mod adjacency_tests {
    use super::*;

    /// `neighbors_in` is now just a walk — the reach was decided when the
    /// sequence was built (see `views::build_adjacency`, and the tests
    /// there for what the sequence contains).
    #[test]
    fn walks_the_sequence_in_both_directions() {
        let seq = [3, 2, 1, 0];
        let (newer, older) = neighbors_in(&seq, 2);
        assert_eq!((newer, older), (Some(3), Some(1)));

        // The ends terminate rather than wrap.
        assert_eq!(neighbors_in(&seq, 3), (None, Some(2)));
        assert_eq!(neighbors_in(&seq, 0), (Some(1), None));

        // A row absent from the sequence has no neighbours at all — which
        // is how a filtered-out row (a draft, under a declared set) stops
        // appearing as someone's later post.
        assert_eq!(neighbors_in(&seq, 9), (None, None));
    }
}

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

#[derive(Debug, Default, Serialize)]
pub struct ObjectsTable {
    pub rows: Table<Row>,
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
    /// For a `*` view: the ROUTES it selected, as positions into
    /// `SiteDb::routes`.
    ///
    /// Separate from `members` rather than sharing it, because the two index
    /// different stores and a caller cannot tell which from the field alone.
    /// A test that walked every route's `members` into the row store found
    /// that out by panicking, which is the good outcome and not one to rely
    /// on twice.
    #[serde(skip)]
    pub route_members: Vec<usize>,
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
    pub fn new(url: String, kind: RouteKind) -> Route {
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
            route_members: Vec::new(),
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
    /// Every content row, posts and tree alike. One table since q51 — the
    /// two that preceded it held the same type and differed only in which
    /// loader filled them.
    pub rows: Table<Row>,
    /// Indices of rows a posts collection produced. This is what "the posts
    /// table" meant: a posts view ranges over ALL of them, across every
    /// posts collection, and `published` narrows by FLAG rather than by
    /// source. Deleting the table meant naming that set instead of
    /// pointing at a `Vec`.
    #[serde(skip)]
    pub post_ix: Vec<usize>,
    /// Indices of rows the tree loader produced.
    #[serde(skip)]
    pub page_ix: Vec<usize>,
    /// The primary index (DESIGN.md §3): `(date, slug)`, unique.
    /// NOT `slug` alone — measured: `not-dead-yet` is used by both a 2003
    /// and a 2006 post, legal because their dates (and so URLs) differ.
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
    /// Safe to share across both origins now that `logical` is
    /// root-relative on each.
    #[serde(skip)]
    pub by_logical: BTreeMap<String, Vec<usize>>,
    pub objects: ObjectsTable,
    pub routes: Table<Route>,
    /// Row sets for views that resolve to exactly one — the ones with no route
    /// to hang `members` on: named queries (`published`) and embedded views
    /// (`latest`). Grouped and paginated views resolve to many sets, which live
    /// on their routes instead (DESIGN.md §5c).
    pub views: BTreeMap<String, ViewRows>,
    /// Root-relative directories containing a `.section` scope marker (§6e):
    /// each roots a section tree its rendered rows carry. Engine vocabulary
    /// like `.slots/` — no config entry names it.
    pub sections: Vec<PathBuf>,
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
    // Which collection claimed the file. Queryable so that a set can name
    // its rows without `from` naming a table — the thing standing between
    // one row store and one `published` set over all of it.
    s.insert("collection", Str);
    // The file itself, which every row is one of. `object_schema` had these
    // and the row schema did not, back when an object was a different type.
    s.insert("name", Str);
    s.insert("ext", Str);
    s.insert("size", Int);
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
            "collection" => V::Str(self.collection.clone()),
            "name" => self
                .rel
                .file_name()
                .map_or(V::Null, |s| V::Str(s.to_string_lossy().to_string())),
            "ext" => self.rel.extension().map_or(V::Str(String::new()), |s| {
                V::Str(s.to_string_lossy().to_lowercase())
            }),
            "size" => V::Int(self.size as i64),
            // Schema fields (§5b) resolve after the base names — the same
            // fallthrough a page has had.
            other => self.fields.get(other).cloned().unwrap_or(V::Null),
        }
    }
}

/// Fields a filter may reference on an OBJECT row.
///
/// An object is a `Row` like any other now, so this is not a different type's
/// schema — it is a narrower query vocabulary for a table whose rows carry no
/// front matter. Keeping it narrow is the point: `where = "draft"` on a
/// gallery is a load error naming the view, rather than a filter that matches
/// nothing because every object's `draft` is false.
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

// ------------------------------------------------------------- insertion

impl SiteDb {
    /// The row a route points at, whichever table holds it. Row identity is
    /// one thing since q51; only the storage is still two.
    pub fn row_by_url(&self, url: &str) -> Option<&Row> {
        self.by_url.get(url).map(|&i| &self.rows[i])
    }

    /// The rows a posts collection produced, in load order.
    pub fn posts(&self) -> impl Iterator<Item = &Row> {
        self.post_ix.iter().map(|&i| &self.rows[i])
    }

    /// Seed a database with rows of one origin, indexing by URL only.
    ///
    /// A test fixture, and public because the tests that want it now live in
    /// other crates. Prefer `insert_rows`, which is what a real load uses:
    /// this one skips the dated indexes and every uniqueness constraint, so a
    /// fixture can hold rows a site could not.
    pub fn seed(rows: Vec<Row>, posts: bool) -> SiteDb {
        let ix: Vec<usize> = (0..rows.len()).collect();
        let mut db = SiteDb {
            rows: Table::new(rows),
            ..Default::default()
        };
        if posts {
            db.post_ix = ix;
        } else {
            db.page_ix = ix;
        }
        for (i, r) in db.rows.iter().enumerate() {
            if !r.url.is_empty() {
                db.by_url.insert(r.url.clone(), i);
            }
        }
        db
    }

    /// The rows the tree loader produced.
    pub fn pages(&self) -> impl Iterator<Item = &Row> {
        self.page_ix.iter().map(|&i| &self.rows[i])
    }

    /// Fill the row store from its two origins and build every index.
    ///
    /// The one way rows enter the database. `posts` and `pages` arrive
    /// already ordered — the loader decides load order, since it is the half
    /// that knows what a collection is.
    ///
    /// `default_locale` is the only configuration fact the database needs,
    /// for one rule (§6f): the dated indexes are single-locale, because a
    /// translation shares its original's `(date, slug)` by design. Passed
    /// rather than read, so the database keeps no opinion about where
    /// configuration lives.
    pub fn insert_rows(
        &mut self,
        posts: Vec<Row>,
        pages: Vec<Row>,
        default_locale: &str,
    ) -> Result<()> {
        // ONE row store (q51). Posts first, then the tree — the split
        // survives only as two index lists, which is what "the posts table"
        // always meant: a set of rows, not a separate type.
        self.post_ix = (0..posts.len()).collect();
        self.page_ix = (posts.len()..posts.len() + pages.len()).collect();
        self.rows = Table::new(posts);
        self.rows.extend(pages);
        self.index_rows(default_locale)
    }

    /// Every index, built once over the whole row store. Was two routines
    /// against two vectors; the constraints they enforced were the same
    /// constraints, and a URL collision between a post and a page was the
    /// one neither could see (q51).
    ///
    /// Each index is its key function: what a row contributes, and what it
    /// means for a row to contribute nothing. `grackle_db::index` owns the
    /// rest — the collision rule and the grouping.
    fn index_rows(&mut self, default_locale: &str) -> Result<()> {
        // A claimed row serves a landing and has no route (q45), so it holds
        // no URL to index.
        let by_url = self
            .rows
            .unique_index(|_, p| (!p.url.is_empty()).then(|| p.url.clone()));

        // A static file has no logical identity to pair a translation on.
        self.by_logical = self
            .rows
            .multi_index(|_, p| p.rendered.then(|| p.logical.clone()));

        // The dated indexes are posts-only and single-locale (§6f): a
        // translation shares its original's (date, slug) by design.
        let n_posts = self.post_ix.len();
        let dated = |i: usize, p: &Row| i < n_posts && p.locale == default_locale;

        let by_key = self
            .rows
            .unique_index(|i, p| dated(i, p).then(|| (p.date, p.slug.clone())));
        self.by_slug = self
            .rows
            .multi_index(|i, p| dated(i, p).then(|| p.slug.clone()));
        self.by_tag = self.rows.multi_index(|i, p| {
            if dated(i, p) {
                p.tags.clone()
            } else {
                Vec::new()
            }
        });
        self.by_year_month = self
            .rows
            .multi_index(|i, p| dated(i, p).then(|| p.year_month()).flatten());

        // Resolved after the borrows above, because naming a collision needs
        // the rows the index layer only knows as positions.
        self.by_url =
            by_url.map_err(|c| self.collision(&format!("route collision at {}:", c.key), c))?;
        self.by_key = by_key.map_err(|c| {
            let (date, slug) = &c.key;
            let date = date.map(|d| d.to_string()).unwrap_or("none".into());
            self.collision(
                &format!("duplicate (date, slug) key ({date}, {slug:?}):"),
                c,
            )
        })?;
        Ok(())
    }

    /// Name the two rows that claimed one key. The index layer reports
    /// positions, having no idea a row has a path to blame.
    fn collision<K>(&self, what: &str, c: grackle_db::Collision<K>) -> anyhow::Error {
        anyhow::anyhow!(
            "{what}\n  {}\n  {}",
            self.rows[c.first].path.display(),
            self.rows[c.second].path.display()
        )
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

#[cfg(test)]
mod row_column_tests {
    use super::*;
    use filter::Row as _;
    use std::path::PathBuf;

    fn row(rel: &str) -> Row {
        Row {
            rel: PathBuf::from(rel),
            size: 42,
            ..Default::default()
        }
    }

    /// `name`, `ext` and `dir` all read the path the row already stores, so
    /// they cost no fields and cannot disagree with it.
    #[test]
    fn the_file_columns_derive_from_the_path() {
        let r = row("photos/holiday/beach.JPG");
        assert_eq!(r.field("name"), filter::Value::Str("beach.JPG".into()));
        assert_eq!(r.field("dir"), filter::Value::Str("photos/holiday".into()));
        assert_eq!(r.field("size"), filter::Value::Int(42));
    }

    /// Lowercased, because the objects loader matches `extensions` case
    /// -insensitively and a query should agree with what got claimed.
    #[test]
    fn ext_is_lowercased_and_empty_when_absent() {
        assert_eq!(
            row("a/b.JPG").field("ext"),
            filter::Value::Str("jpg".into())
        );
        assert_eq!(
            row("a/README").field("ext"),
            filter::Value::Str(String::new())
        );
    }

    /// The collection is what a set will name once `from` stops naming a
    /// table — a row store spanning posts and tree needs a column saying
    /// which claimed each row.
    #[test]
    fn collection_is_queryable() {
        let r = Row {
            collection: "notes".into(),
            ..Default::default()
        };
        assert_eq!(r.field("collection"), filter::Value::Str("notes".into()));
        assert!(row_schema().contains_key("collection"));
    }

    /// Every column `object_schema` names must be answerable by a `Row`,
    /// since an object row IS one — the narrower schema is a vocabulary, not
    /// a different type.
    #[test]
    fn a_row_answers_every_object_column() {
        let r = row("photos/beach.jpg");
        for col in object_schema().keys() {
            assert_ne!(
                r.field(col),
                filter::Value::Null,
                "object column {col:?} is unanswerable on a Row"
            );
        }
    }
}

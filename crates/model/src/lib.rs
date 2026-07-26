//! The site's data model: what a row, an object and a route are, the fields a
//! filter may name on each, and the indexes and constraints over them.
//!
//! Everything domain-specific about grack.com lives here. The machinery it is
//! built from — the filter language, the index shapes — is `grackle-db`, which
//! knows none of it. Rows arrive through `SiteDb::insert_rows`; the layer that
//! produces them is `grackle-source`.

use grackle_db::{filter, Keyed, Table};

pub use grackle_db::Key;

use anyhow::{bail, Result};
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

#[derive(Debug, Default, Serialize)]
pub struct Row {
    /// What this row IS, as opposed to where it currently sits. Assigned by
    /// `insert_rows` from `rel`, because a row's source file is the one thing
    /// about it that survives a rebuild.
    pub key: Key,
    /// The collection whose source claimed this file. Relations anchor to
    /// it, not to the table: two dated collections in one table interleave,
    /// making a blog post's "later post" a note.
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
    /// §5g).
    pub theme: Option<String>,
    pub shell: Option<String>,
    /// Typed extra fields, validated against the governing `.schema.toml`
    /// (§5b). Declarations come from a root-wide walk, so they are visible
    /// to every row whatever loader filled it.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, filter::Value>,
    /// The image-typed subset of `fields`: field name -> the value, kept
    /// apart because only the loader knows which fields `.schema.toml`
    /// declared as images and the renderer still has to find them.
    ///
    /// Each value is checked at load to name a row of this site, or to be an
    /// absolute url naming something outside it (`resolve_image_fields`) — so
    /// a relative one here is a reference that resolves, not a hopeful string.
    #[serde(skip)]
    pub images: BTreeMap<String, String>,
    /// Declared position (§6e). A post's *table* order is chronological;
    /// this is what a view sorts on when it says so — see `order_by` in
    /// `build_views`.
    pub order: Option<i64>,
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
    /// §4: this row's route rule was `on_demand`, so it publishes only when
    /// something references it. The URL is computed either way — what is
    /// deferred is whether a `Route` exists — which is what lets a link
    /// resolve to a row nothing has materialized yet.
    pub on_demand: bool,
    pub size: u64,
    /// An object's pixel shape, header-read at load beside `size` (§6b's
    /// dimension facts, q26). A file property like any other, so a view can
    /// ask for it: `where = "width > height"` selects the landscape ones.
    /// `None` for a row that is not an image, or one whose header would not
    /// parse.
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// q45: this row is a landing view's content — no standalone route,
    /// excluded from every query structurally.
    #[serde(skip)]
    pub claimed: bool,
    /// q53: the axis this row's route rule spends, if any.
    #[serde(skip)]
    pub axis: Option<RowAxis>,
}

impl Keyed for Route {
    fn key(&self) -> &Key {
        &self.id
    }
}

impl Keyed for Row {
    fn key(&self) -> &Key {
        &self.key
    }
}

impl Row {
    pub fn year_month(&self) -> Option<(i32, u32)> {
        use chrono::Datelike;
        self.date.map(|d| (d.year(), d.month()))
    }

    /// A declared `bool` field, false when the site never declared it (§4e).
    ///
    /// Engine code should reach for this **rarely and namelessly**: a flag is
    /// site vocabulary, and the remaining callers that spell one out — the
    /// `noindex` head fact, the inspector, `explain` — are exactly what
    /// `[html.head.meta]` and a generic field dump are meant to delete.
    pub fn flag(&self, name: &str) -> bool {
        matches!(self.fields.get(name), Some(filter::Value::Bool(true)))
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

#[cfg(test)]
mod adjacency_tests {
    use super::*;

    /// `neighbors_in` is just a walk — the reach was decided when the
    /// sequence was built (see `views::build_adjacency`).
    #[test]
    fn walks_the_sequence_in_both_directions() {
        let k = |s: &str| Key::new(s);
        let seq = [k("d"), k("c"), k("b"), k("a")];
        assert_eq!(
            neighbors_in(&seq, &k("c")),
            (Some(k("d")), Some(k("b"))),
            "position in the sequence is what newer and older mean"
        );

        // The ends terminate rather than wrap.
        assert_eq!(neighbors_in(&seq, &k("d")), (None, Some(k("c"))));
        assert_eq!(neighbors_in(&seq, &k("a")), (Some(k("b")), None));

        // A row absent from the sequence has no neighbours at all — which
        // is how a filtered-out row (a draft, under a declared set) stops
        // appearing as someone's later post.
        assert_eq!(neighbors_in(&seq, &k("gone")), (None, None));
    }
}

/// Walk a prepared sequence. The sequence IS the reach (q51), so there is
/// no collection filter here: `db.adjacency` is built per collection, and a
/// declared `adjacency` set carries its own `from`. A sequence spanning
/// collections makes a blog post's neighbour a note.
pub fn neighbors_in(seq: &[Key], of: &Key) -> (Option<Key>, Option<Key>) {
    let Some(pos) = seq.iter().position(|k| k == of) else {
        return (None, None);
    };
    (
        seq[..pos].iter().next_back().cloned(),
        seq.get(pos + 1).cloned(),
    )
}

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

/// The axis a ROW's route rule spends (q53 step 2), and the URL template that
/// spends it — `/{theme}/notes/{slug}/` with the axis segment still unspent.
///
/// The rule that spends an axis is the rule that opts its rows in: `[axes.*]`
/// declares values and a field, and the URL shape lives where every other URL
/// shape lives. `Row.url` is the CANONICAL member's, so links, `by_url` and
/// every reader that wants "the address of this row" get the right answer
/// without knowing an axis exists.
#[derive(Debug, Clone, Serialize)]
pub struct RowAxis {
    pub name: String,
    pub template: String,
}

/// One route's membership of an axis (q53): which axis, which value, and the
/// row field that value sets while rendering.
///
/// `field` is carried rather than looked up so the render paths need no handle
/// on the config to ask "what does this member wear" — the same reason a
/// group key carries its params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxisMember {
    pub axis: String,
    pub value: String,
    pub field: String,
    /// The first-declared member, which is what `rel="canonical"` names and the
    /// only one a `*` view sees. An alternate is not a duplicate; it is what
    /// `rel="alternate"` is for.
    pub canonical: bool,
}

#[derive(Debug, Serialize)]
pub struct Route {
    /// A route's identity is its URL: unique by the collision check at load,
    /// and the same string across rebuilds. Not `key` — that is the GROUP
    /// key a subdivided view wears, which is a different thing entirely.
    #[serde(skip)]
    pub id: Key,
    pub url: String,
    pub kind: RouteKind,
    /// The row this route renders, for the routes that render one (`Post`,
    /// `Page`, `Static`, `Object`). `None` for a view's routes, which render a
    /// query rather than a row.
    ///
    /// The row→route relation, stated. It used to be recovered by looking the
    /// route's URL up in `by_url`, which works only while a row has exactly one
    /// URL — an assumption held by every per-row map in the renderer and by
    /// nothing in the design. Carrying the key means a second route onto one row
    /// (q53's axis) is a thing the renderer can express rather than a thing it
    /// silently cannot.
    #[serde(skip)]
    pub row: Option<Key>,
    /// Which axis member this route is (q53), `None` for the ordinary case of
    /// a row published once. Two routes onto one row are legal exactly when
    /// they are different members of one axis — see §4's constraint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub axis: Option<AxisMember>,
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
    /// The source row's DECLARED fields, carried onto the route so a `*` view
    /// (the sitemap) can select on them. Without this the sitemap's filter
    /// language has no way to say "not a draft" and a future draft would leak
    /// into the most public URL there is (DESIGN.md §4a).
    ///
    /// A map rather than the two named bools it replaces, because the flag
    /// family is ordinary declared schema now (§4e): whatever a site declares
    /// is what a star view may filter on, and the engine names none of it.
    #[serde(skip)]
    pub fields: BTreeMap<String, filter::Value>,
    /// For a `*` view: the ROUTES it selected.
    ///
    /// Separate from `members` rather than sharing it, because the two name
    /// rows in different stores and a caller cannot tell which from the
    /// field alone.
    #[serde(skip)]
    pub route_members: Vec<Key>,
    /// `self`: the post rows this route materializes, in order.
    ///
    /// The view's declared query decides these once, here — renderers read
    /// them rather than re-deriving. Empty for `over = "*"` views, which
    /// range over routes rather than posts.
    #[serde(skip)]
    pub members: Vec<Key>,
}

impl Route {
    /// A route with nothing but a URL and a kind — the base every
    /// constructor site fills its few meaningful fields over.
    pub fn new(url: String, kind: RouteKind) -> Route {
        Route {
            id: Key::new(&url),
            url,
            kind,
            row: None,
            axis: None,
            source: None,
            view: None,
            key: None,
            rows: None,
            page: None,
            params: Vec::new(),
            locale: None,
            fields: BTreeMap::new(),
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
/// Does NOT expose `noindex`: computing it needs the layout chain, and a field
/// that cannot be populated correctly is worse than no field — referencing it
/// is a load-time error rather than a silent lie. jekyll-sitemap ignores
/// noindex anyway, so nothing wants it.
pub fn route_schema(declared: &filter::Schema) -> filter::Schema {
    use filter::Type::*;
    let mut s = filter::Schema::new();
    s.insert("kind", Str);
    s.insert("view", Str);
    s.insert("url", Str);
    s.insert("ext", Str);
    s.insert("dir", Bool);
    // Whatever the site declared reaches the route, `draft`/`hidden` included
    // — they are declared fields like any other now (§4e), so this vocabulary
    // is the site's rather than the engine's.
    for (k, t) in declared {
        s.insert(k, *t);
    }
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
            // Declared fields carried from the source row (§4e) — the same
            // fallthrough `Row` has, so `draft` reads the same at both layers.
            other => self.fields.get(other).cloned().unwrap_or(V::Null),
        }
    }
}

#[derive(Debug, Default, Serialize)]
pub struct SiteDb {
    /// Every content row, posts and tree alike.
    pub rows: Table<Row>,
    /// The fields this site declared (§4e), as a filter schema. Part of the
    /// database because it is what the database's rows actually answer to —
    /// `draft` is in here, not in `row_schema()`, and a consumer that wants to
    /// parse a filter needs the site's vocabulary rather than the engine's.
    #[serde(skip)]
    pub declared: filter::Schema,
    /// Keys of rows a posts collection produced. A posts view ranges over
    /// ALL of them, across every posts collection; `published` narrows by
    /// FLAG rather than by source.
    #[serde(skip)]
    pub post_ix: Vec<Key>,
    /// Indices of rows the tree loader produced.
    #[serde(skip)]
    pub page_ix: Vec<Key>,
    /// The primary index (DESIGN.md §3): `(date, slug)`, unique.
    /// NOT `slug` alone — measured: `not-dead-yet` is used by both a 2003
    /// and a 2006 post, legal because their dates (and so URLs) differ.
    #[serde(skip)]
    pub by_key: HashMap<(Option<NaiveDate>, String), Key>,
    /// Non-unique: slug -> rows. Informational; see `by_key` for identity.
    #[serde(skip)]
    pub by_slug: BTreeMap<String, Vec<Key>>,
    #[serde(skip)]
    pub by_tag: BTreeMap<String, Vec<Key>>,
    #[serde(skip)]
    pub by_year_month: BTreeMap<(i32, u32), Vec<Key>>,
    #[serde(skip)]
    pub by_url: HashMap<String, Key>,
    /// §6f: logical identity -> every locale variant (default included).
    /// Safe to share across both origins now that `logical` is
    /// root-relative on each.
    #[serde(skip)]
    pub by_logical: BTreeMap<String, Vec<Key>>,
    /// Keys of rows an objects collection produced. Every index in
    /// `index_rows` gates on row PROPERTIES (`post_ix` membership,
    /// `rendered`), never on which origin a row arrived from.
    #[serde(skip)]
    pub object_ix: Vec<Key>,
    /// Object basename -> rows. Deliberately non-unique (DESIGN.md §6a):
    /// `screenshot5.png` genuinely collides, so resolution is a query that
    /// can fail rather than a map lookup.
    ///
    /// NOTE: §6a's bubble+bucket bare-name resolution is **specced, not
    /// built** — nothing reads this except `query stats`, and `[objects]
    /// bucket` is parsed and never read. `{% image %}` joins its literal
    /// argument to the root, so a bare name errors rather than resolving.
    #[serde(skip)]
    pub by_name: BTreeMap<String, Vec<Key>>,
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
    /// first.
    #[serde(skip)]
    pub adjacency: BTreeMap<String, Vec<Key>>,
    /// Declared neighbour queries per collection (§6g), in dependency order —
    /// each `earlier`/`later`/`related`/`linked_from`/custom relation compiled
    /// from config and type-checked against the two-row schema at load. The
    /// engine evaluates them per row at build. Replaces the hardcoded axes and
    /// the `adjacency` sequence.
    #[serde(skip)]
    pub relations: BTreeMap<String, Vec<Relation>>,
    pub stats: LoadStats,
}

/// The engine-provided relation names (§6g "graph and path are names"): a
/// pool or a membership set the config may reference without declaring. They
/// exist for every row whether or not anything renders them, so they are part
/// of the two-row environment's vocabulary.
pub const DERIVED_RELATIONS: &[&str] = &[
    "links_to",
    "linked_from",
    "ancestors",
    "parent",
    "children",
    "siblings",
    "descendants",
];

/// A compiled relation (§6g): a neighbour query over the two-row environment.
/// The expression ASTs are parsed and type-checked at load; the engine walks
/// candidates through `over → where → rank (+min_rank) → limit` per row.
#[derive(Debug, Clone)]
pub struct Relation {
    pub name: String,
    /// The candidate pool. A set/collection is row-independent; a derived
    /// name (`linked_from`) is row-relative — the difference the engine
    /// resolves per row.
    pub pool: Pool,
    /// Which `self` rows carry this relation (the `match` glob), already
    /// compiled. `None` = every row of the collection.
    pub scope: Option<globset::GlobMatcher>,
    pub filter: filter::Filter,
    pub rank: Option<filter::Rank>,
    pub min_rank: Option<f64>,
    pub limit: usize,
    pub label: RelLabel,
}

/// Where a relation draws candidates from.
#[derive(Debug, Clone)]
pub enum Pool {
    /// A named set or route (`published`) — its resolved `members`.
    Set(String),
    /// A collection's rows.
    Collection(String),
    /// A derived name (`linked_from`, `ancestors`, …) — computed per row.
    Derived(String),
}

/// A relation group's heading. Resolved at render into the row's locale: a
/// `Key` reads `[i18n.strings]` (defaulting to the relation's own name), the
/// other two are used verbatim. Kept free of the config's `LocalizedStr` so
/// the model owns no config types.
#[derive(Debug, Clone)]
pub enum RelLabel {
    /// An `@ref` into the string table (or the default, the relation name).
    Key(String),
    /// A single literal, for a monolingual site.
    Text(String),
    /// A per-locale literal map.
    PerLocale(BTreeMap<String, String>),
}

/// The two-row environment's schema (§6g): every base field under both
/// `self.` and `candidate.` prefixes, the bare `self`/`candidate` as the rows'
/// URLs, and every relation name (derived + declared) as a list. This is the
/// CEL environment a relation `where`/`rank` type-checks against.
pub fn two_row_schema(base: &filter::Schema, relation_names: &[String]) -> filter::Schema {
    let mut s = filter::Schema::new();
    for (name, ty) in base {
        s.insert(intern(format!("self.{name}")), *ty);
        s.insert(intern(format!("candidate.{name}")), *ty);
    }
    // The rows themselves, as URLs — the left of `candidate in earlier` and
    // the arguments to the score functions.
    s.insert("self", filter::Type::Str);
    s.insert("candidate", filter::Type::Str);
    for n in DERIVED_RELATIONS {
        s.insert(n, filter::Type::List);
    }
    for n in relation_names {
        s.insert(intern(n.clone()), filter::Type::List);
    }
    s
}

/// A `filter::Schema` keys on `&'static str`, but a relation schema needs
/// computed keys (`self.date`). Leaking each would grow unbounded across
/// `serve` reloads; interning bounds it to the finite set of distinct field
/// names, however many times the config reloads.
pub fn intern(s: String) -> &'static str {
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};
    static POOL: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let pool = POOL.get_or_init(|| Mutex::new(HashSet::new()));
    let mut set = pool.lock().unwrap();
    if let Some(existing) = set.get(s.as_str()) {
        return existing;
    }
    let leaked: &'static str = Box::leak(s.into_boxed_str());
    set.insert(leaked);
    leaked
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
    pub members: Vec<Key>,
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

/// Fields a filter may reference on a row, and their types. Everything else
/// is a load-time error (filter.rs), so a typo can't silently match
/// everything.
pub fn row_schema() -> filter::Schema {
    use filter::Type::*;
    let mut s = filter::Schema::new();
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
    s.insert("toc", Bool);
    s.insert("tags", List);
    s.insert("rendered", Bool);
    s.insert("path", Str);
    s.insert("dir", Str);
    // §6f: the row's locale, always set (the default when no selector fired).
    s.insert("locale", Str);
    // Which collection claimed the file. Queryable so that a set can name
    // its rows without `from` naming a table — the thing standing between
    // one row store and one `published` set over all of it.
    s.insert("collection", Str);
    // The file itself, which every row is one of.
    s.insert("name", Str);
    s.insert("ext", Str);
    s.insert("size", Int);
    s.insert("width", Int);
    s.insert("height", Int);
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
            // `rel` is root-relative for every row, whatever its origin.
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
            "width" => self.width.map_or(V::Null, |w| V::Int(w as i64)),
            "height" => self.height.map_or(V::Null, |h| V::Int(h as i64)),
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
    s.insert("width", Int);
    s.insert("height", Int);
    s
}

impl SiteDb {
    /// The row a route points at.
    pub fn row_by_url(&self, url: &str) -> Option<&Row> {
        self.by_url.get(url).and_then(|k| self.rows.get(k))
    }

    /// The row a key names.
    pub fn row(&self, key: &Key) -> Option<&Row> {
        self.rows.get(key)
    }

    /// The rows a posts collection produced, in load order.
    pub fn posts(&self) -> impl Iterator<Item = &Row> {
        self.post_ix.iter().filter_map(|k| self.rows.get(k))
    }

    /// Seed a database with rows of one origin, indexing by URL only.
    ///
    /// A test fixture, and public because the tests that want it now live in
    /// other crates. Prefer `insert_rows`, which is what a real load uses:
    /// this one skips the dated indexes and every uniqueness constraint, so a
    /// fixture can hold rows a site could not.
    pub fn seed(rows: Vec<Row>, posts: bool) -> SiteDb {
        let mut rows = rows;
        for r in rows.iter_mut() {
            r.key = Key::new(r.rel.to_string_lossy());
        }
        let ix: Vec<Key> = rows.iter().map(|r| r.key.clone()).collect();
        let mut db = SiteDb {
            rows: Table::new(rows),
            ..Default::default()
        };
        if posts {
            db.post_ix = ix;
        } else {
            db.page_ix = ix;
        }
        let urls: Vec<(String, Key)> = db
            .rows
            .iter()
            .filter(|r| !r.url.is_empty())
            .map(|r| (r.url.clone(), r.key.clone()))
            .collect();
        db.by_url.extend(urls);
        db
    }

    /// The rows the tree loader produced.
    pub fn pages(&self) -> impl Iterator<Item = &Row> {
        self.page_ix.iter().filter_map(|k| self.rows.get(k))
    }

    /// The rows an objects collection produced: binaries, never rendered.
    pub fn objects(&self) -> impl Iterator<Item = &Row> {
        self.object_ix.iter().filter_map(|k| self.rows.get(k))
    }

    /// Fill the row store from its three origins and build every index.
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
        mut posts: Vec<Row>,
        mut pages: Vec<Row>,
        mut objects: Vec<Row>,
        default_locale: &str,
    ) -> Result<()> {
        // Keys are assigned here, where rows stop being the loader's and
        // become the database's. A row's key is its source file.
        for r in posts
            .iter_mut()
            .chain(pages.iter_mut())
            .chain(objects.iter_mut())
        {
            r.key = Key::new(r.rel.to_string_lossy());
        }
        self.post_ix = posts.iter().map(|r| r.key.clone()).collect();
        self.page_ix = pages.iter().map(|r| r.key.clone()).collect();
        self.object_ix = objects.iter().map(|r| r.key.clone()).collect();
        self.by_name = objects.iter().fold(BTreeMap::new(), |mut m, r| {
            let name = r
                .rel
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            m.entry(name).or_insert_with(Vec::new).push(r.key.clone());
            m
        });
        self.rows = Table::new(posts);
        self.rows.extend(pages);
        self.rows.extend(objects);
        self.index_rows(default_locale)
    }

    /// Every index, built once over the whole row store — which is what makes
    /// a URL collision between a post and a page visible (q51).
    ///
    /// Each index is its key function: what a row contributes, and what it
    /// means for a row to contribute nothing. `grackle_db::index` owns the
    /// rest — the collision rule and the grouping.
    fn index_rows(&mut self, default_locale: &str) -> Result<()> {
        // Identity first: two rows sharing a key is a corpus that cannot be
        // indexed by key at all, and the table would silently resolve one of
        // them. Checked before anything depends on it.
        let mut seen: HashMap<&Key, &Row> = HashMap::new();
        for p in self.rows.iter() {
            if let Some(prev) = seen.insert(&p.key, p) {
                bail!(
                    "duplicate row key {}:\n  {}\n  {}",
                    p.key,
                    prev.path.display(),
                    p.path.display()
                );
            }
        }

        // Posts-only and single-locale (§6f): a translation shares its
        // original's (date, slug) by design.
        //
        // Membership, not arithmetic: nothing here may depend on posts being
        // laid down first.
        let posts: std::collections::HashSet<&Key> = self.post_ix.iter().collect();
        let dated = |p: &Row| posts.contains(&p.key) && p.locale == default_locale;

        // A claimed row serves a landing and has no route (q45), so it holds
        // no URL to index.
        let by_url = self
            .rows
            .unique_index(|p| (!p.url.is_empty()).then(|| p.url.clone()));
        let by_key = self
            .rows
            .unique_index(|p| dated(p).then(|| (p.date, p.slug.clone())));
        // A static file has no logical identity to pair a translation on.
        let by_logical = self
            .rows
            .multi_index(|p| p.rendered.then(|| p.logical.clone()));
        let by_slug = self.rows.multi_index(|p| dated(p).then(|| p.slug.clone()));
        let by_tag = self
            .rows
            .multi_index(|p| if dated(p) { p.tags.clone() } else { Vec::new() });
        let by_year_month = self
            .rows
            .multi_index(|p| dated(p).then(|| p.year_month()).flatten());

        self.by_logical = by_logical;
        self.by_slug = by_slug;
        self.by_tag = by_tag;
        self.by_year_month = by_year_month;
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

    /// Name the two rows that claimed one key. The index layer reports keys,
    /// having no idea a row has a path to blame.
    fn collision<K>(&self, what: &str, c: grackle_db::Collision<K>) -> anyhow::Error {
        let path = |k: &Key| {
            self.rows
                .get(k)
                .map(|r| r.path.display().to_string())
                .unwrap_or_else(|| k.to_string())
        };
        anyhow::anyhow!("{what}\n  {}\n  {}", path(&c.first), path(&c.second))
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
        let f = Filter::parse("stem != \"index\"", &route_schema(&filter::Schema::new())).unwrap();
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
        let mut r = row("photos/beach.jpg");
        // Measured, because dimensions are legitimately absent on a row that
        // is not an image — the assertion below is about a column the type
        // cannot answer AT ALL, not about one this fixture left empty.
        r.width = Some(1200);
        r.height = Some(800);
        for col in object_schema().keys() {
            assert_ne!(
                r.field(col),
                filter::Value::Null,
                "object column {col:?} is unanswerable on a Row"
            );
        }
    }

    /// An unmeasured row answers Null, not zero: `where = "width >= 400"`
    /// must skip the rows that have no pixels rather than treat them as
    /// zero-width and compare them.
    #[test]
    fn an_unmeasured_row_has_null_dimensions() {
        let r = row("notes/x.md");
        assert_eq!(r.field("width"), filter::Value::Null);
        assert_eq!(r.field("height"), filter::Value::Null);
    }
}

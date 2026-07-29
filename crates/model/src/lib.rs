//! The site's data model: what a row, an object and a route are, the fields a
//! filter may name on each, and the indexes and constraints over them.
//!
//! Everything domain-specific about grack.com lives here. The machinery it is
//! built from — the filter language, the index shapes — is `grackle-db`, which
//! knows none of it. Rows arrive through `SiteDb::insert_rows`; the layer that
//! produces them is `grackle-source`.
//!
//! Structs live one-per-file; enums stay here.

use grackle_db::filter;

pub mod alternate;
pub mod ask;
pub mod axis_member;
pub mod edge;
pub mod graph;
pub mod heading;
pub mod image_tag;
pub mod load_stats;
pub mod outline_node;
pub mod relation;
pub mod rendition;
pub mod route;
pub mod row;
pub mod row_axis;
pub mod site_db;
pub mod view_rows;

pub use alternate::Alternate;
pub use ask::Ask;
pub use axis_member::AxisMember;
pub use edge::Edge;
pub use grackle_db::Key;
pub use heading::Heading;
pub use image_tag::ImageTag;
pub use load_stats::LoadStats;
pub use outline_node::OutlineNode;
pub use relation::Relation;
pub use rendition::Rendition;
pub use route::Route;
pub use row::Row;
pub use row_axis::RowAxis;
pub use site_db::SiteDb;
pub use view_rows::ViewRows;

use chrono::NaiveDate;
use serde::Serialize;
use std::collections::BTreeMap;

/// Citation form: link vs embed (IO.md §4a).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cite {
    /// `[text](x)`, `<a href>` — an address a human may bookmark.
    Link,
    /// `![alt](x)`, `<img src>`, `<iframe src>` — bytes the page pulls in.
    Embed,
}

/// Which HTML head element a declared meta binding emits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    /// `<meta name=… content=…>`
    Meta,
    /// `<meta property=… content=…>`
    Property,
    /// `<link rel=… href=…>`
    Link,
}

/// A node: one row of the inputs database, or one row of the outputs database.
///
/// Two variants rather than one key space, because the two stores key
/// differently — an input by its source path, an output by its URL — and a
/// bare key cannot say which table it came from. The graph's whole job is to
/// hold both at once.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Node {
    Input(Key),
    Output(Key),
}

impl Node {
    pub fn key(&self) -> &Key {
        match self {
            Node::Input(k) | Node::Output(k) => k,
        }
    }

    /// How a node prints in a diagnostic: the key, tagged with its side.
    pub fn label(&self) -> String {
        match self {
            Node::Input(k) => format!("input {k}"),
            Node::Output(k) => format!("output {k}"),
        }
    }
}

/// What a dependent demands of a dependency — §1's law, as an edge label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Demand {
    /// The dependent's **bytes** read the dependency. Its content stage has to
    /// be forced before this output can materialize, so this is the edge that
    /// orders work — and the only kind a cycle could ever be made of.
    Content,
    /// The dependent reads the dependency's **planning facts** only (url,
    /// shell, declared fields). Complete for every output before any content
    /// exists, so it orders nothing.
    Facts,
}

/// The output's kind — **the last table tag, and what is left of it** after
/// IO.md I13 (*delete `kind`*) took what was honestly takeable.
///
/// §3's claim is that this enum is a flattened product of independent facts
/// (`front_mattered`, `output`, `shell`, the join, scope membership, view
/// provenance) and should dissolve into them. Measured at I13, it dissolves
/// **partly**, and the census is worth carrying at the definition because the
/// reasons differ per survivor:
///
/// - **`View` is fully respellable and was respelled.** "Is this a view route"
///   is the `view` column being non-empty — the three sites that mint one all
///   set it and nothing else does — so all eight `kind == View` tests in the
///   engine now read `view`, and three of them turned out to be asking the
///   same question twice and were deleted outright.
/// - **`Post` vs `Page` is NOT respellable**, and the reason is I9's, one
///   store over: "this scope's role is posts" is a statement about CONFIG, and
///   a row carries `collection` — the scope's *name* — and nothing that says
///   what kind of scope that was. Adding a bit to make it expressible would
///   re-mint, on the output side, exactly the origin distinction I7e deleted.
///   The two render in different passes from different body stores, so
///   `build.rs`'s render dispatch and `search_pass`'s doc arms read the enum.
/// - **`Static` vs `Object` is respellable and no engine path asks it.** Both
///   are byte copies (`Static | Object` is one arm wherever it is dispatched
///   on, and it equals `!rendered` on all six corpus trees — measured), and
///   the only reader that tells the two apart is the `kind` column itself.
/// - **The COLUMN cannot go**, which is what keeps the enum alive whatever the
///   engine does internally: grack.com's `[routes.search]` and its
///   `[profiles.drafts]` restatement filter `kind == "post"`, meaning the blog
///   corpus — SCOPE MEMBERSHIP — and the route pool has no column for that.
///   The migration is Matt's call with an expressibility item as its
///   prerequisite (§3's re-pointed marker). Until then this is a live config
///   vocabulary, `NAMES` is its domain, and `check_domain` is what keeps the
///   silent-empty-query knife (`kind == "posts"`) a load error.
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

    /// Every value the `kind` column can hold — the closed domain a `where`
    /// comparing against it is checked against (IO.md §3, item I1), so that
    /// `kind == "posts"` is a load error rather than a filter that matches
    /// nothing for as long as the config lives.
    ///
    /// A second spelling of the enum, held to it by the
    /// `every_variant_is_in_the_kind_domain` test below — whose `match` stops
    /// compiling the day a variant is added.
    pub const NAMES: &'static [&'static str] = &["post", "page", "static", "object", "view"];
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

/// A relation group's heading. Resolved at render for the row's i18n member: a
/// `Key` reads `[i18n.strings]` (defaulting to the relation's own name), the
/// other two are used verbatim. Kept free of the config's `LocalizedStr` so
/// the model owns no config types.
#[derive(Debug, Clone)]
pub enum RelLabel {
    /// An `@ref` into the string table (or the default, the relation name).
    Key(String),
    /// A single literal, for a monolingual site.
    Text(String),
    /// A per-member literal map.
    PerLocale(BTreeMap<String, String>),
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

/// `2022-03-16` — sortable; the machine-readable date everywhere.
pub fn iso_date(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// `16 March 2022` — what themes and search hits show.
pub fn pretty_date(d: NaiveDate) -> String {
    d.format("%-d %B %Y").to_string()
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

/// Fields a filter may reference on a route.
///
/// Does NOT expose `noindex`: computing it needs the layout chain, and a field
/// that cannot be populated correctly is worse than no field — referencing it
/// is a load-time error rather than a silent lie. jekyll-sitemap ignores
/// noindex anyway, so nothing wants it.
///
/// **Nor `inputs`** (IO.md I9), by that same rule rather than by a new one.
/// The one filter the engine runs over this pool is a fold's `where`, in
/// `resolve_pool_folds` — and a fold's own membership is what completes the
/// edge set, so at the moment that filter runs `inputs` is either empty or
/// half-filled depending on view name order. A column no filter can read
/// correctly is not a column; `inputs` is a field of the outputs table with
/// `grackle explain` as its surface, and I10's graph as its consumer.
pub fn route_schema(declared: &filter::Schema) -> filter::Schema {
    use filter::Type::*;
    let mut s = filter::Schema::new();
    // Closed domain (IO.md I1): the column is a `Str` backed by an enum, so
    // the engine knows every value it can hold and a comparison against
    // anything else is a load error naming the knowns. The fossil is safe
    // while it dies.
    s.insert("kind", Enum(RouteKind::NAMES));
    // IO.md §3: did the input this output came from carry a front-matter
    // block? **False, not Null, for a view route** — a view has no source
    // file, so it carried nothing, and a fold over the route pool needs a
    // predicate that is total rather than one that answers "not applicable"
    // to a third of the table.
    s.insert("front_mattered", Bool);
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
    s
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

/// Fields a filter may reference on a row, and their types. Everything else
/// is a load-time error (filter.rs), so a typo can't silently match
/// everything.
pub fn row_schema() -> filter::Schema {
    use filter::Type::*;
    let mut s = filter::Schema::new();
    s.insert("title", Str);
    s.insert("slug", Str);
    s.insert("stem", Str);
    s.insert("description", Str);
    s.insert("url", Str);
    // ISO-8601, so string ordering is date ordering.
    s.insert("date", Str);
    s.insert("year", Int);
    s.insert("month", Int);
    s.insert("day", Int);
    s.insert("body_bytes", Int);
    s.insert("order", Int);
    s.insert("rendered", Bool);
    // IO.md §3: has identity — the row's file carried a front-matter block
    // (and, from I8, a sidecar counts). Distinct from `rendered`, which says
    // the pipeline parsed it; see `Row::front_mattered`.
    s.insert("front_mattered", Bool);
    // IO.md §2, the join. `output` is a RECORD, and this language has no
    // record type — so it enters as the honest pair the language does have: a
    // bool saying the record exists, and a dotted column per field it
    // projects. `date.year` is the same spelling one construct over (see
    // `spec_field`), and the dotted name costs nothing because an identifier
    // may already contain a `.`.
    //
    // The pair, not one column carrying both jobs: a `Str` holding the URL
    // would make `output == "/x/"` type-check, which reads as comparing a
    // record to a string. `!output` is the landings exclusion said out loud
    // (q45's claimed rows), and `output.url` is the address.
    //
    // Complete before any view filter runs — routes are minted first — which
    // is what separates these from `viewed_by` and `inputs`.
    s.insert("output", Bool);
    s.insert("output.url", Str);
    // The `rel="alternate"` set (q53): this row's other forms, as URLs. A
    // planning fact like `output`, so it is safe to select on.
    s.insert("alternates", List);
    // NOT `viewed_by`: it is what a view's membership PRODUCES, so at the
    // moment a view's `where` is evaluated it is empty for every row —
    // `route_schema`'s `noindex` rule ("a field that cannot be populated
    // correctly is worse than no field"), one table over. Selection may not
    // read arrangement. `grackle explain` prints it; relations, which run at
    // build, would be able to read it, and get it the day something needs it.
    s.insert("path", Str);
    s.insert("dir", Str);
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

#[cfg(test)]
mod route_stem_tests {
    use super::*;
    use crate::filter::Filter;
    use std::path::PathBuf;

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

    /// `RouteKind::NAMES` is the `kind` column's value domain (IO.md I1), and
    /// it is a second spelling of the enum — so something has to hold the two
    /// together.
    ///
    /// The `match` is that something: add a variant and this test stops
    /// COMPILING, and the fix is one line in each place. Without it a new
    /// kind would be a value the domain rejects, which turns a working filter
    /// into a load error — the failure mode of a checker that is wrong rather
    /// than absent.
    #[test]
    fn every_variant_is_in_the_kind_domain() {
        let all = [
            RouteKind::Post,
            RouteKind::Page,
            RouteKind::Static,
            RouteKind::Object,
            RouteKind::View,
        ];
        for k in all {
            match k {
                RouteKind::Post
                | RouteKind::Page
                | RouteKind::Static
                | RouteKind::Object
                | RouteKind::View => {}
            }
            assert!(
                RouteKind::NAMES.contains(&k.as_str()),
                "{} is a kind the domain does not know",
                k.as_str()
            );
        }
        assert_eq!(RouteKind::NAMES.len(), all.len(), "and nothing extra");
    }

    /// IO.md §3, the output side: a view route has no source file, so it
    /// carried no front matter — `false`, not Null, because a fold over the
    /// route pool needs a total predicate (`!front_mattered` must mean
    /// something on every row of the table).
    #[test]
    fn a_view_route_is_not_front_mattered() {
        let f = Filter::parse("front_mattered", &route_schema(&filter::Schema::new())).unwrap();
        let doc = Route {
            front_mattered: true,
            ..Route::new("/recipes/carbonara/".into(), RouteKind::Page)
        };
        assert!(f.eval(&doc));
        assert!(!f.eval(&Route::new("/blog/".into(), RouteKind::View)));
        // And a byte copy, which is the other half of the item's question.
        assert!(!f.eval(&Route::new("/logo.png".into(), RouteKind::Object)));
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

    /// Lowercased, because a rule's `match` glob compiles case-insensitively
    /// (IO.md I7a) and a query should agree with what got claimed.
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

    /// Tags is a declared list in `fields`, not a built-in row column.
    #[test]
    fn tags_is_not_a_built_in_row_field() {
        assert!(!row_schema().contains_key("tags"));
        let mut r = Row::default();
        r.fields
            .insert("tags".into(), filter::Value::List(vec!["x".into()]));
        assert_eq!(r.field("tags"), filter::Value::List(vec!["x".into()]));
        assert_eq!(r.list("tags"), vec!["x".to_string()]);
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

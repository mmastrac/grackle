//! Site data model: rows, routes, indexes, and the fields filters may name.
//!
//! Domain types live here. Filter machinery is `grackle-db`; loading is
//! `grackle-source`. Structs are one file each; enums stay in this module.

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
pub use row::{parse_date_str, Row};
pub use row_axis::RowAxis;
pub use site_db::SiteDb;
pub use view_rows::ViewRows;

use chrono::NaiveDate;
use serde::Serialize;
use std::collections::BTreeMap;

/// How a citation points at a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cite {
    /// Bookmarkable address (`[text](…)`, `<a href>`).
    Link,
    /// Inlined bytes (`![…](…)`, `<img>`, `<iframe>`).
    Embed,
}

/// HTML head element kind for a declared meta binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    /// `<meta name=… content=…>`
    Meta,
    /// `<meta property=… content=…>`
    Property,
    /// `<link rel=… href=…>`
    Link,
}

/// A graph node: an input row or an output route.
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

    /// Key with its side, for diagnostics.
    pub fn label(&self) -> String {
        match self {
            Node::Input(k) => format!("input {k}"),
            Node::Output(k) => format!("output {k}"),
        }
    }
}

/// What a dependent needs from a dependency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Demand {
    /// Needs the dependency's bytes (orders work; can form cycles).
    Content,
    /// Needs planning facts only (url, shell, fields). Does not order work.
    Facts,
}

/// Kind of published route. Also the closed domain of the `kind` filter column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RouteKind {
    Page,
    Static,
    Object,
    View,
}

impl RouteKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RouteKind::Page => "page",
            RouteKind::Static => "static",
            RouteKind::Object => "object",
            RouteKind::View => "view",
        }
    }

    /// Values the `kind` column may hold.
    pub const NAMES: &'static [&'static str] = &["page", "static", "object", "view"];
}

/// Where a relation draws candidates from.
#[derive(Debug, Clone)]
pub enum Pool {
    /// A named set or route (`published`).
    Set(String),
    /// A collection's rows.
    Collection(String),
    /// A built-in name (`linked_from`, `ancestors`, …), computed per row.
    Derived(String),
}

/// Heading for a relation group, resolved at render for the row's pairing member.
#[derive(Debug, Clone)]
pub enum RelLabel {
    /// Key into `[i18n.strings]`, falling back to the relation name.
    Key(String),
    /// A single literal string.
    Text(String),
    /// Literal per pairing-axis member.
    PerMember(BTreeMap<String, String>),
}

/// Declared date-typed fields: names that also have a `{name}.year` companion
/// in the filter schema (see [`crate::source::schema`] date expansion).
pub fn date_fields(declared: &filter::Schema) -> Vec<&'static str> {
    let years: std::collections::HashSet<&str> = declared
        .keys()
        .copied()
        .filter_map(|k| k.strip_suffix(".year"))
        .collect();
    let mut out: Vec<_> = declared
        .keys()
        .copied()
        .filter(|n| !n.contains('.') && years.contains(*n))
        .collect();
    out.sort_unstable();
    out
}

/// Machine-readable date (`2022-03-16`).
pub fn iso_date(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// Previous and next keys around `of` in an ordered sequence.
pub fn neighbors_in(seq: &[Key], of: &Key) -> (Option<Key>, Option<Key>) {
    let Some(pos) = seq.iter().position(|k| k == of) else {
        return (None, None);
    };
    (
        seq[..pos].iter().next_back().cloned(),
        seq.get(pos + 1).cloned(),
    )
}

/// Filter fields available on a route.
///
/// Omits `noindex` and `inputs`: those cannot be filled correctly when fold
/// filters run.
pub fn route_schema(declared: &filter::Schema) -> filter::Schema {
    use filter::Type::*;
    let mut s = filter::Schema::new();
    s.insert("kind", Enum(RouteKind::NAMES));
    s.insert("front_mattered", Bool);
    s.insert("collection", Str);
    s.insert("view", Str);
    s.insert("url", Str);
    s.insert("ext", Str);
    s.insert("dir", Bool);
    for (k, t) in declared {
        s.insert(k, *t);
    }
    s.insert("key", Str);
    s.insert("page", Int);
    s.insert("rows", Int);
    s.insert("stem", Str);
    s
}

/// Built-in relation names available without a config declaration.
pub const DERIVED_RELATIONS: &[&str] = &[
    "links_to",
    "linked_from",
    "ancestors",
    "parent",
    "children",
    "siblings",
    "descendants",
];

/// Schema for a relation `where` / `rank`: base fields under `self.` /
/// `candidate.`, plus bare URLs and relation names as lists.
pub fn two_row_schema(base: &filter::Schema, relation_names: &[String]) -> filter::Schema {
    let mut s = filter::Schema::new();
    for (name, ty) in base {
        s.insert(intern(format!("self.{name}")), *ty);
        s.insert(intern(format!("candidate.{name}")), *ty);
    }
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

/// Intern a computed schema key (`self.date`, …) so reloads stay bounded.
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

/// Filter fields available on a row.
pub fn row_schema() -> filter::Schema {
    use filter::Type::*;
    let mut s = filter::Schema::new();
    s.insert("title", Str);
    s.insert("slug", Str);
    s.insert("stem", Str);
    s.insert("url", Str);
    s.insert("body_bytes", Int);
    s.insert("order", Int);
    s.insert("rendered", Bool);
    s.insert("front_mattered", Bool);
    s.insert("output", Bool);
    s.insert("output.url", Str);
    s.insert("alternates", List);
    s.insert("path", Str);
    s.insert("dir", Str);
    s.insert("collection", Str);
    s.insert("name", Str);
    s.insert("ext", Str);
    // Metadata namespaces: `.file.*` (filesystem), `.image.*` (decoded image
    // header, absent off images), `.git.*` (last commit, absent unless
    // `metadata = ["git"]`). Each is a provider over the row, `Null` where it
    // has nothing to say.
    s.insert("file.size", Int);
    s.insert("image.width", Int);
    s.insert("image.height", Int);
    s.insert("git.commit", Str);
    s.insert("git.lastmod", Str);
    s.insert("git.author", Str);
    s.insert("git.comment", Str);
    s
}

#[cfg(test)]
mod adjacency_tests {
    use super::*;

    #[test]
    fn walks_the_sequence_in_both_directions() {
        let k = |s: &str| Key::new(s);
        let seq = [k("d"), k("c"), k("b"), k("a")];
        assert_eq!(
            neighbors_in(&seq, &k("c")),
            (Some(k("d")), Some(k("b"))),
            "position in the sequence is what newer and older mean"
        );

        assert_eq!(neighbors_in(&seq, &k("d")), (None, Some(k("c"))));
        assert_eq!(neighbors_in(&seq, &k("a")), (Some(k("b")), None));

        assert_eq!(neighbors_in(&seq, &k("gone")), (None, None));
    }
}

#[cfg(test)]
mod route_stem_tests {
    use super::*;
    use crate::filter::Filter;
    use std::path::PathBuf;

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

    /// `NAMES` must stay in lockstep with the enum variants.
    #[test]
    fn every_variant_is_in_the_kind_domain() {
        let all = [
            RouteKind::Page,
            RouteKind::Static,
            RouteKind::Object,
            RouteKind::View,
        ];
        for k in all {
            match k {
                RouteKind::Page | RouteKind::Static | RouteKind::Object | RouteKind::View => {}
            }
            assert!(
                RouteKind::NAMES.contains(&k.as_str()),
                "{} is a kind the domain does not know",
                k.as_str()
            );
        }
        assert_eq!(RouteKind::NAMES.len(), all.len(), "and nothing extra");
    }

    #[test]
    fn a_view_route_is_not_front_mattered() {
        let f = Filter::parse("front_mattered", &route_schema(&filter::Schema::new())).unwrap();
        let doc = Route {
            front_mattered: true,
            ..Route::new("/recipes/carbonara/".into(), RouteKind::Page)
        };
        assert!(f.eval(&doc));
        assert!(!f.eval(&Route::new("/blog/".into(), RouteKind::View)));
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
            width: Some(800),
            height: Some(600),
            ..Default::default()
        }
    }

    #[test]
    fn the_file_columns_derive_from_the_path() {
        let r = row("photos/holiday/beach.JPG");
        assert_eq!(r.field("name"), filter::Value::Str("beach.JPG".into()));
        assert_eq!(r.field("dir"), filter::Value::Str("photos/holiday".into()));
    }

    #[test]
    fn file_and_image_metadata_are_namespaced() {
        use filter::Value as V;
        let img = row("photos/beach.jpg");
        assert_eq!(img.field("file.size"), V::Int(42));
        assert_eq!(img.field("image.width"), V::Int(800));
        assert_eq!(img.field("image.height"), V::Int(600));
        // The flat names moved under their namespace.
        assert_eq!(img.field("size"), V::Null);
        assert_eq!(img.field("width"), V::Null);
        assert_eq!(img.field("height"), V::Null);
        // `.image.*` is Null off an image.
        let doc = Row {
            size: 10,
            ..Default::default()
        };
        assert_eq!(doc.field("image.width"), V::Null);
        assert_eq!(doc.field("file.size"), V::Int(10));
    }

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

    #[test]
    fn tags_is_not_a_built_in_row_field() {
        assert!(!row_schema().contains_key("tags"));
        let mut r = Row::default();
        r.fields
            .insert("tags".into(), filter::Value::str_list(vec!["x".into()]));
        assert_eq!(r.field("tags"), filter::Value::str_list(vec!["x".into()]));
        assert_eq!(r.list("tags"), vec!["x".to_string()]);
    }

    #[test]
    fn collection_is_queryable() {
        let r = Row {
            collection: "notes".into(),
            ..Default::default()
        };
        assert_eq!(r.field("collection"), filter::Value::Str("notes".into()));
        assert!(row_schema().contains_key("collection"));
    }
}

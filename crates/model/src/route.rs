//! Output-side route: one published URL.

use crate::{AxisMember, Key, Rendition, RouteKind};
use grackle_db::{filter, Keyed};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Serialize)]
pub struct Route {
    /// A route's identity is its URL: unique by the collision check at load,
    /// and the same string across rebuilds. Not `key` — that is the GROUP
    /// key a subdivided view wears, which is a different thing entirely.
    #[serde(skip)]
    pub id: Key,
    pub url: String,
    /// IO.md §4a: this output's **hash address**, when the embed policy
    /// published it — [`Row::strong_url`] on the output side.
    ///
    /// For an output the policy minted, this equals `url`: the strong address
    /// is where the artifact landed, because no rule gave it another one.
    /// That equality is the honest reading of "strong addresses are the
    /// content store made public, not routes" — the node exists so
    /// invalidation can reach it and the pull can order it, and the *rule*
    /// that would have minted a canonical URL was never written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strong_url: Option<String>,
    pub kind: RouteKind,
    /// IO.md §3: the input this output came from carried a front-matter block.
    ///
    /// Copied from the row rather than derived from `kind`, because the two
    /// genuinely disagree: a `.md` in a posts scope with no front matter is a
    /// `Post` route all the same (the scope grants it a date, a slug and a
    /// URL), and grack.com has one. `kind == "post"` is scope membership;
    /// this is identity.
    ///
    /// `false` for a view route — it has no source file, so there was nothing
    /// to carry.
    pub front_mattered: bool,
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
    /// Which axis members this route is (q53) — the tuple of members, one per
    /// axis the route spends, empty for the ordinary case of a row published
    /// once. Two routes onto one row are legal exactly when they differ in this
    /// tuple, which is what lets several axes compose into the product rather
    /// than collide — see §4's constraint.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub axis: Vec<AxisMember>,
    /// q45 mode B, per route: the resolved logical path of the row this route
    /// embeds as its landing body, set when the view's `content` is a TEMPLATE
    /// (`{group:key}/index.md`) and so resolves to a different row per route, or
    /// when a templated `default_content` route accepted its offer. `None` for a
    /// literal content claim, which stays view-level on `View.content`, and for
    /// an ordinary route.
    #[serde(skip)]
    pub content: Option<String>,
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
    /// The source row's DECLARED fields, carried onto the route so a `*` view
    /// (the sitemap) can select on them. Without this the sitemap's filter
    /// language has no way to say "not a draft" and a future draft would leak
    /// into the most public URL there is (DESIGN.md §4a).
    ///
    /// A map rather than the two named bools it replaces, because the flag
    /// family is ordinary declared schema now (§4e): whatever a site declares
    /// is what an all-outputs fold may filter on, and the engine names none
    /// of it. **Locale** is stamped here when non-default (§6f) — not an engine
    /// column; [`Route::locale`] reads it weakly so alternates and
    /// locale-parallel views keep working until the row-side built-in dissolves.
    #[serde(skip)]
    pub fields: BTreeMap<String, filter::Value>,
    /// **The outputs this output reads the FACTS of** — the output→output half
    /// of the graph, and `graph::Demand::Facts` is what the column means.
    ///
    /// Two populations, and they are the same relation rather than two:
    ///
    /// - for a `*` view, the ROUTES it selected (I9);
    /// - for any output whose finished bytes embed a rendition, that rendition
    ///   (I12) — the citing edge of IO.md §4a. It is a facts edge because what
    ///   the page read is the rendition's *address*, and the hashing law makes
    ///   an address a planning fact.
    ///
    /// Separate from `members` rather than sharing it, because the two name
    /// rows in different stores and a caller cannot tell which from the
    /// field alone.
    #[serde(skip)]
    pub route_members: Vec<Key>,
    /// `self`: the post rows this route materializes, in order.
    ///
    /// The view's declared query decides these once, here — renderers read
    /// them rather than re-deriving. Empty for a fold with no `from` (IO.md
    /// §4), which ranges over routes rather than posts.
    #[serde(skip)]
    pub members: Vec<Key>,
    /// IO.md §2, the join's output side: every input row that fed this
    /// output, keyed by source path, sorted and deduped.
    ///
    /// **The invalidation edge set, as a column.** The incremental machinery's
    /// typed keys have been curating exactly these edges by hand; this is the
    /// same set said once, in the row store's own vocabulary. Wiring
    /// invalidation to it is I10's — this field is the edge, not yet the
    /// mechanism.
    ///
    /// Scope is the **full row-level closure** (IO.md §2's `[open]`, decided
    /// at I9): the row a route renders, a landing's claimed content row, a
    /// view's members, the source rows behind a pool fold's selected routes,
    /// and — added at render, because a citation is a fact about content —
    /// every row the finished bytes cite. Non-row dependencies (theme files,
    /// `.slots/` fills, config) are NOT here: they are not rows, so they stay
    /// the existing typed keys, which is what "row-level closure" narrows to
    /// by construction.
    ///
    /// The output→output half of the same graph is `route_members`: a fold
    /// over the route pool arranges outputs, and `inputs` then holds the rows
    /// behind them.
    #[serde(skip)]
    pub inputs: Vec<Key>,
    /// IO.md §4a, I12: this output is a **rendition** — a transform of its
    /// `inputs` — and these are the transform's parameters.
    ///
    /// **The home demand-carried parameters got** (review I-D's question).
    /// They are not on the edge: a rendition's address hashes the input bytes
    /// *plus* these, so every content edge arriving here carries the same
    /// parameters by construction, and a slot on the edge would hold N copies
    /// of one value with nothing keeping them equal. `rendition::Rendition`'s
    /// module doc carries the argument.
    ///
    /// `None` for every output that is not a transform of something — which is
    /// every output the corpus had before this item.
    #[serde(skip)]
    pub rendition: Option<Rendition>,
}

impl Keyed for Route {
    fn key(&self) -> &Key {
        &self.id
    }
}

impl Route {
    /// A route with nothing but a URL and a kind — the base every
    /// constructor site fills its few meaningful fields over.
    pub fn new(url: String, kind: RouteKind) -> Route {
        Route {
            id: Key::new(&url),
            url,
            strong_url: None,
            kind,
            front_mattered: false,
            row: None,
            axis: Vec::new(),
            content: None,
            source: None,
            view: None,
            key: None,
            rows: None,
            page: None,
            params: Vec::new(),
            fields: BTreeMap::new(),
            members: Vec::new(),
            route_members: Vec::new(),
            inputs: Vec::new(),
            rendition: None,
        }
    }

    /// The route's locale, when a non-default one was stamped into [`fields`]
    /// (§6f). Weak: the engine does not own the name — sites that decline the
    /// base and never declare `locale` simply have none. Alternates and
    /// locale-parallel views read through this.
    pub fn locale(&self) -> Option<&str> {
        match self.fields.get("locale") {
            Some(filter::Value::Str(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Stamp a non-default locale into [`fields`]. No-op for the default, so
    /// filters keep seeing Null there.
    pub fn set_locale(&mut self, locale: Option<String>) {
        match locale {
            Some(l) => {
                self.fields.insert("locale".into(), filter::Value::Str(l));
            }
            None => {
                self.fields.remove("locale");
            }
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

impl filter::Row for Route {
    fn field(&self, name: &str) -> filter::Value {
        use filter::Value as V;
        match name {
            "kind" => V::Str(self.kind.as_str().to_string()),
            "front_mattered" => V::Bool(self.front_mattered),
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
                    let logical = match self.locale() {
                        Some(l) => s
                            .strip_suffix(l)
                            .and_then(|rest| rest.strip_suffix('.'))
                            .unwrap_or(s.as_ref()),
                        None => s.as_ref(),
                    };
                    V::Str(logical.to_owned())
                }),
            // Declared fields carried from the source row (§4e) — the same
            // fallthrough `Row` has, so `draft` / `locale` read the same at
            // both layers.
            other => self.fields.get(other).cloned().unwrap_or(V::Null),
        }
    }
}

//! The site database: rows, routes, indexes, and load stats.

use crate::{Key, LoadStats, Relation, Route, Row, ViewRows};
use anyhow::{bail, Result};
use chrono::NaiveDate;
use grackle_db::{filter, Table};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

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
    /// List-field value → rows, nested by field name. One map per declared
    /// `type = "list"` field (tags, course, ...); within-key order is table order.
    #[serde(skip)]
    pub by_multi_key: BTreeMap<String, BTreeMap<String, Vec<Key>>>,
    #[serde(skip)]
    pub by_year_month: BTreeMap<(i32, u32), Vec<Key>>,
    #[serde(skip)]
    pub by_url: HashMap<String, Key>,
    /// IO.md §4a, I11: **strong address → the rows that share it.**
    ///
    /// `by_url` is the CANONICAL address index and holds canonical row URLs
    /// only, so a `/static/{hash}` citation resolves to nothing there — which
    /// is exactly the hole review I-D named: an embedding page's `inputs`
    /// would silently lose the asset edge, and the pull would never publish
    /// the bytes. This is the other half of citation resolution.
    ///
    /// **Non-unique on purpose**, and that is the untransformed-twin rule
    /// spelled as a data structure: the address is a pure function of the
    /// bytes, so two inputs holding one byte string legitimately share one
    /// address and one store entry. A unique index would have called that a
    /// collision; a multi-index calls it dedupe, which is what it is.
    #[serde(skip)]
    pub by_strong: BTreeMap<String, Vec<Key>>,
    /// §6f: logical identity → every file-axis twin (canonical included).
    /// Safe to share across both origins now that `logical` is
    /// root-relative on each.
    #[serde(skip)]
    pub by_logical: BTreeMap<String, Vec<Key>>,
    /// Keys of the rows that are PICTURES — the extension fact (IO.md I7e):
    /// an objects scope's globs claim the path, which is I7a's rule-claimed
    /// membership. Not "which loader made the row": there is one row
    /// constructor, and every index in `index_rows` gates on row PROPERTIES
    /// (`post_ix` membership, `rendered`) for the same reason.
    #[serde(skip)]
    pub object_ix: Vec<Key>,
    /// Object basename -> rows. Deliberately non-unique (DESIGN.md §6a):
    /// `screenshot5.png` genuinely collides, so resolution is a query that
    /// can fail rather than a map lookup.
    ///
    /// NOTE: §6a's bubble+bucket bare-name resolution is **specced and
    /// parked** (MERGE.md F1, 2026-07-27 — the `[objects] bucket` key it
    /// would have read is deleted). `{% image %}` joins its literal argument
    /// to the root, so a bare name errors rather than resolving. This index
    /// stays because it has a live reader of its own: `query stats` reports
    /// the distinct-name and ambiguous-name counts off it, which is the
    /// measurement §6a's collision argument rests on.
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
    /// unset — every row of the collection on the i18n canonical, newest
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
    /// What the load had to say but would not fail over — today, DESIGN.md
    /// §4's dead rules. They are printed by the loader as they are found;
    /// they are also kept here because a warning nothing can read is a
    /// warning no test can hold the loader to.
    #[serde(skip)]
    pub warnings: Vec<String>,
    /// Rung 0's forced route fields (§4a, MERGE.md E1), typed once at load and
    /// kept so a route minted LATER can take them too.
    ///
    /// `load::force_route_fields` writes every route that exists when it runs;
    /// `build::materialize_referenced` mints one after the load has returned
    /// (IO.md §4's pull model), and a route the profile never reached would be
    /// a rung-0 hole that grows every time an output is minted at a new seam.
    /// One typed list, two writers — the alternative is re-deriving the values
    /// at the second seam from a `Schemas` the build does not have.
    #[serde(skip)]
    pub forced_fields: BTreeMap<String, filter::Value>,
    pub stats: LoadStats,
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

    /// The pictures: the rows the extension fact names (see `object_ix`).
    pub fn objects(&self) -> impl Iterator<Item = &Row> {
        self.object_ix.iter().filter_map(|k| self.rows.get(k))
    }

    /// Fill the row store from the loader's three key lists and build every
    /// index.
    ///
    /// The one way rows enter the database. The three arguments are not three
    /// origins — since IO.md I7e one constructor builds every row and the
    /// lists are a partition of its output, each keyed off a fact: `posts` is
    /// the claiming scope's role, `objects` is the extension fact, `pages` is
    /// the rest. They arrive already ordered — the loader decides load order,
    /// since it is the half that knows what a collection is.
    ///
    /// **The three vectors STAY a shape** (IO.md I9, claiming I7e's flag —
    /// "the join is where it either becomes a query or stays"). The join gave
    /// the argument rather than the capability, and it goes both ways at once:
    ///
    /// - A query needs its predicate in the row's own columns, and neither
    ///   fact is there. "This scope's role is posts" and "an objects scope's
    ///   glob claimed this path" are both statements about CONFIG — a row
    ///   carries `collection`, the scope's *name*, and nothing that says what
    ///   kind of scope that was. Adding the two bits to make the query
    ///   expressible would re-mint, as two engine-named row facts, exactly the
    ///   origin distinction I7e deleted.
    /// - A query returns a set; this hands over a SEQUENCE. `post_ix`'s order
    ///   is load order after `sort_posts`, and it is load-bearing: `embed`'s
    ///   vectors are parallel to it, `relate` reads them by that position, and
    ///   `by_multi_key`/`by_year_month`/`by_slug` take their within-key order from
    ///   the table's. Ordering-derived bytes, and no predicate carries an
    ///   order.
    ///
    /// So the boundary is right where it is: the loader knows config and
    /// decides order, the database owns identity and the indexes, and the
    /// three lists are the handover.
    ///
    /// `dated_keep` is `(field, canonical)` for the pairing axis when a site
    /// has one: dated indexes keep that axis's canonical only, because a
    /// twin shares its original's `(date, slug)` by design (§6f). `None`
    /// when there is no pairing axis (monolingual / no `[i18n] axis`).
    pub fn insert_rows(
        &mut self,
        mut posts: Vec<Row>,
        mut pages: Vec<Row>,
        mut objects: Vec<Row>,
        dated_keep: Option<(&str, &str)>,
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
        self.index_rows(dated_keep)
    }

    /// Every index, built once over the whole row store — which is what makes
    /// a URL collision between a post and a page visible (q51).
    ///
    /// Each index is its key function: what a row contributes, and what it
    /// means for a row to contribute nothing. `grackle_db::index` owns the
    /// rest — the collision rule and the grouping.
    fn index_rows(&mut self, dated_keep: Option<(&str, &str)>) -> Result<()> {
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

        // Posts-only, pairing-axis canonical (§6f): a twin shares its
        // original's (date, slug) by design.
        //
        // Membership, not arithmetic: nothing here may depend on posts being
        // laid down first.
        let posts: std::collections::HashSet<&Key> = self.post_ix.iter().collect();
        let dated = |p: &Row| {
            posts.contains(&p.key)
                && match dated_keep {
                    Some((field, canon)) => p.string(field) == Some(canon),
                    None => true,
                }
        };

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
        let mut by_multi_key = BTreeMap::new();
        for (field, ty) in &self.declared {
            if *ty != filter::Type::List {
                continue;
            }
            let field = *field;
            by_multi_key.insert(
                field.to_string(),
                self.rows.multi_index(|p| {
                    if !dated(p) {
                        return Vec::new();
                    }
                    match filter::Row::field(p, field) {
                        filter::Value::List(v) => v,
                        _ => Vec::new(),
                    }
                }),
            );
        }
        let by_year_month = self
            .rows
            .multi_index(|p| dated(p).then(|| p.year_month()).flatten());
        // The second address index (IO.md §4a): MULTI, because sharing an
        // address is what identical bytes are supposed to do.
        let by_strong = self.rows.multi_index(|p| p.strong_url.clone());

        self.by_strong = by_strong;
        self.by_logical = by_logical;
        self.by_slug = by_slug;
        self.by_multi_key = by_multi_key;
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

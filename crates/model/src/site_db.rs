//! Site database: rows, routes, indexes, and load stats.

use crate::{Key, LoadStats, Relation, Route, Row, ViewRows};
use anyhow::{bail, Result};
use chrono::NaiveDate;
use grackle_db::{filter, Table};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

#[derive(Debug, Default, Serialize)]
pub struct SiteDb {
    /// All content rows.
    pub rows: Table<Row>,
    /// Site-declared field schema (what filters may name beyond the engine set).
    #[serde(skip)]
    pub declared: filter::Schema,
    /// Rows from posts collections, in load order.
    #[serde(skip)]
    pub post_ix: Vec<Key>,
    /// Rows from the tree (non-posts, non-objects).
    #[serde(skip)]
    pub page_ix: Vec<Key>,
    /// Unique `(date, slug)` -> row.
    #[serde(skip)]
    pub by_key: HashMap<(Option<NaiveDate>, String), Key>,
    /// Slug -> rows (not unique).
    #[serde(skip)]
    pub by_slug: BTreeMap<String, Vec<Key>>,
    /// Declared list field -> value -> rows.
    #[serde(skip)]
    pub by_multi_key: BTreeMap<String, BTreeMap<String, Vec<Key>>>,
    #[serde(skip)]
    pub by_year_month: BTreeMap<(i32, u32), Vec<Key>>,
    /// Canonical URL -> row.
    #[serde(skip)]
    pub by_url: HashMap<String, Key>,
    /// Hash address -> rows that share it (identical bytes may share one address).
    #[serde(skip)]
    pub by_strong: BTreeMap<String, Vec<Key>>,
    /// Logical identity -> file-axis twins.
    #[serde(skip)]
    pub by_logical: BTreeMap<String, Vec<Key>>,
    /// Image / object rows.
    #[serde(skip)]
    pub object_ix: Vec<Key>,
    /// Basename -> object rows (may collide).
    #[serde(skip)]
    pub by_name: BTreeMap<String, Vec<Key>>,
    pub routes: Table<Route>,
    /// Routeless views (named sets, embeds).
    pub views: BTreeMap<String, ViewRows>,
    /// Directories marked with `.section`.
    pub sections: Vec<PathBuf>,
    /// Prev/next sequence per posts collection.
    #[serde(skip)]
    pub adjacency: BTreeMap<String, Vec<Key>>,
    /// Compiled neighbour queries per collection.
    #[serde(skip)]
    pub relations: BTreeMap<String, Vec<Relation>>,
    /// Non-fatal load messages.
    #[serde(skip)]
    pub warnings: Vec<String>,
    /// Profile-forced route fields, kept for routes minted after load.
    #[serde(skip)]
    pub forced_fields: BTreeMap<String, filter::Value>,
    pub stats: LoadStats,
}

impl SiteDb {
    /// Row at a URL, if any.
    pub fn row_by_url(&self, url: &str) -> Option<&Row> {
        self.by_url.get(url).and_then(|k| self.rows.get(k))
    }

    /// Row for a key, if any.
    pub fn row(&self, key: &Key) -> Option<&Row> {
        self.rows.get(key)
    }

    /// Posts-collection rows in load order.
    pub fn posts(&self) -> impl Iterator<Item = &Row> {
        self.post_ix.iter().filter_map(|k| self.rows.get(k))
    }

    /// Test helper: index by URL only, skip uniqueness checks. Prefer [`insert_rows`].
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

    /// Tree rows in load order.
    pub fn pages(&self) -> impl Iterator<Item = &Row> {
        self.page_ix.iter().filter_map(|k| self.rows.get(k))
    }

    /// Object / image rows.
    pub fn objects(&self) -> impl Iterator<Item = &Row> {
        self.object_ix.iter().filter_map(|k| self.rows.get(k))
    }

    /// Load rows and build indexes.
    ///
    /// `dated_keep` is `(field, canonical)` for the pairing axis: dated indexes
    /// keep only that member so twins do not collide on `(date, slug)`. Pass
    /// `None` when there is no pairing axis.
    pub fn insert_rows(
        &mut self,
        mut posts: Vec<Row>,
        mut pages: Vec<Row>,
        mut objects: Vec<Row>,
        dated_keep: Option<(&str, &str)>,
    ) -> Result<()> {
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

    fn index_rows(&mut self, dated_keep: Option<(&str, &str)>) -> Result<()> {
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

        let posts: std::collections::HashSet<&Key> = self.post_ix.iter().collect();
        let dated = |p: &Row| {
            posts.contains(&p.key)
                && match dated_keep {
                    Some((field, canon)) => p.string(field) == Some(canon),
                    None => true,
                }
        };

        let by_url = self
            .rows
            .unique_index(|p| (!p.url.is_empty()).then(|| p.url.clone()));
        let by_key = self
            .rows
            .unique_index(|p| dated(p).then(|| (p.date, p.slug.clone())));
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

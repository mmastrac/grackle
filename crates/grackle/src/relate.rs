//! Evaluating declared relations (DESIGN.md §6g).
//!
//! The config compiled each collection's neighbour lists into dependency-
//! ordered [`Relation`]s (see `grackle-source`'s `relations`). This module is
//! the other half: at build, with the embedding vectors and the link graph in
//! hand, it walks each row's candidates through `over → where → rank → limit`
//! and produces the labelled groups a `document` renders.
//!
//! The environment is two rows — `self` (the row being rendered) and
//! `candidate` — reached through the shared filter language over a
//! [`Pair`]. The relation NAMES resolve to finished lists, built up in the
//! dependency order the loader pinned, so `related`'s `!(candidate in earlier)`
//! reads an `earlier` that is already decided.

use std::collections::HashMap;

use grackle_db::filter::{self, Ctx, Value};
use grackle_model::{Pool, RelLabel, Relation, Row, SiteDb};

use crate::config::Config;
use crate::embed::Vector;

/// The link graph inverted: target URL to the `(title, url, date)` of each row
/// citing it — `build`'s `Backlink` list, borrowed here as the `linked_from`
/// pool.
type Backlinks = HashMap<String, Vec<(String, String, Option<chrono::NaiveDate>)>>;

/// One relation's finished output: a heading and its neighbours, ready for the
/// `relation` part. Items are `(title, url, date)`, the shape `neighbor` wants.
#[derive(Clone)]
pub struct Group {
    pub name: String,
    pub label: String,
    pub items: Vec<(String, String, Option<chrono::NaiveDate>)>,
}

/// The build-time facts a relation reads that no row field carries: embedding
/// vectors and dates by URL, plus the forward/back link graph. Borrows
/// everything — one engine serves the whole render pass.
pub struct Engine<'a> {
    cfg: &'a Config,
    db: &'a SiteDb,
    vec_by_url: HashMap<&'a str, &'a Vector>,
    links_to: &'a HashMap<String, Vec<String>>,
    backlinks: &'a Backlinks,
}

impl<'a> Engine<'a> {
    /// `vectors` is parallel to `db.post_ix` (embed's contract); the rest are
    /// the graph maps `backlinks_map` returns.
    pub fn new(
        cfg: &'a Config,
        db: &'a SiteDb,
        vectors: &'a [Option<Vector>],
        links_to: &'a HashMap<String, Vec<String>>,
        backlinks: &'a Backlinks,
    ) -> Self {
        let mut vec_by_url = HashMap::new();
        for (i, k) in db.post_ix.iter().enumerate() {
            if let (Some(row), Some(Some(v))) = (db.rows.get(k), vectors.get(i)) {
                vec_by_url.insert(row.url.as_str(), v);
            }
        }
        Engine {
            cfg,
            db,
            vec_by_url,
            links_to,
            backlinks,
        }
    }

    /// The relation groups for one row, in declaration order, empties dropped
    /// (hole-algebra rule 2). `None` collection relations ⇒ no groups.
    pub fn groups_for(&self, row: &Row) -> Vec<Group> {
        let Some(rels) = self.db.relations.get(&row.collection) else {
            return Vec::new();
        };
        let ctx = RelCtx { engine: self };
        // The relation NAMES this row resolves — derived names always, then
        // each declared list as it is decided (dependency order guarantees a
        // reference is already present).
        let mut names: HashMap<String, Vec<String>> = self.derived_names(row);
        let mut groups = Vec::new();
        for rel in rels {
            if let Some(scope) = &rel.scope {
                if !scope.is_match(row.rel.to_string_lossy().as_ref()) {
                    continue;
                }
            }
            let members = self.evaluate(row, rel, &names, &ctx);
            names.insert(rel.name.clone(), members.iter().map(|(u, _)| u.clone()).collect());
            if members.is_empty() {
                continue;
            }
            let items = members
                .into_iter()
                .filter_map(|(url, _)| {
                    self.db.row_by_url(&url).map(|r| {
                        (r.title.clone().unwrap_or_default(), r.url.clone(), r.date)
                    })
                })
                .collect();
            groups.push(Group {
                name: rel.name.clone(),
                label: self.label(&rel.label, &row.locale, &rel.name),
                items,
            });
        }
        groups
    }

    /// The engine-provided names for a row (§6g graph + path families).
    fn derived_names(&self, row: &Row) -> HashMap<String, Vec<String>> {
        let mut m = HashMap::new();
        // The link graph, already the citation view (splice excluded).
        m.insert(
            "linked_from".to_string(),
            self.backlinks
                .get(&row.url)
                .map(|v| v.iter().map(|(_, u, _)| u.clone()).collect())
                .unwrap_or_default(),
        );
        m.insert(
            "links_to".to_string(),
            self.links_to.get(&row.url).cloned().unwrap_or_default(),
        );
        // The tree family. `ancestors` is the breadcrumb walk; the rest fall
        // out of URL nesting among rendered rows.
        let anc: Vec<String> = crate::trails::ancestors(self.cfg, self.db, &row.url)
            .into_iter()
            .map(|(u, _)| u)
            .collect();
        m.insert("ancestors".to_string(), anc.clone());
        m.insert(
            "parent".to_string(),
            anc.last().cloned().into_iter().collect(),
        );
        let (children, siblings, descendants) = self.tree_family(row);
        m.insert("children".to_string(), children);
        m.insert("siblings".to_string(), siblings);
        m.insert("descendants".to_string(), descendants);
        m
    }

    /// `children`/`siblings`/`descendants` from URL nesting: a pretty-dir URL
    /// `/a/b/` is the parent of `/a/b/c/`. Derived from the finished route set
    /// rather than a stored tree — nothing on grack.com declares a relation
    /// over these yet, but §6g names them, so they exist.
    fn tree_family(&self, row: &Row) -> (Vec<String>, Vec<String>, Vec<String>) {
        let self_url = row.url.as_str();
        let depth = |u: &str| u.matches('/').count();
        let mut children = Vec::new();
        let mut descendants = Vec::new();
        for r in self.db.rows.iter() {
            if !r.rendered || r.url == row.url || r.locale != row.locale {
                continue;
            }
            if r.url.starts_with(self_url) && self_url.ends_with('/') {
                descendants.push(r.url.clone());
                if depth(&r.url) == depth(self_url) + 1 {
                    children.push(r.url.clone());
                }
            }
        }
        // Siblings share this row's parent URL.
        let parent = crate::trails::ancestors(self.cfg, self.db, self_url)
            .pop()
            .map(|(u, _)| u);
        let siblings = match &parent {
            Some(p) if p.ends_with('/') => self
                .db
                .rows
                .iter()
                .filter(|r| {
                    r.rendered
                        && r.url != row.url
                        && r.locale == row.locale
                        && r.url.starts_with(p.as_str())
                        && depth(&r.url) == depth(p)
                })
                .map(|r| r.url.clone())
                .collect(),
            _ => Vec::new(),
        };
        (children, siblings, descendants)
    }

    /// Walk one relation's candidates: pool → self-excluded, same-locale →
    /// `where` → `rank` (+ `min_rank`) → sort → `limit`. Returns
    /// `(url, score)` best-first.
    fn evaluate(
        &self,
        row: &Row,
        rel: &Relation,
        names: &HashMap<String, Vec<String>>,
        ctx: &RelCtx,
    ) -> Vec<(String, Option<f64>)> {
        let pool = self.pool_rows(rel, names);
        let mut scored: Vec<(String, Option<f64>, Option<chrono::NaiveDate>)> = Vec::new();
        for cand in pool {
            // Self is never a candidate — a mechanism rule, not a per-site
            // `where` clause (§6g).
            if cand.url == row.url {
                continue;
            }
            // §6f: a row's neighbours are in its own language.
            if cand.locale != row.locale {
                continue;
            }
            let pair = Pair {
                self_row: row,
                cand_row: cand,
                names,
            };
            if !rel.filter.eval_ctx(&pair, ctx) {
                continue;
            }
            let score = match &rel.rank {
                Some(rk) => match rk.eval(&pair, ctx) {
                    // An unrankable pair (no vector, undated) drops before the
                    // window rather than sorting to an arbitrary end.
                    None => continue,
                    Some(s) => {
                        if rel.min_rank.is_some_and(|m| s < m) {
                            continue;
                        }
                        Some(s)
                    }
                },
                None => None,
            };
            scored.push((cand.url.clone(), score, cand.date));
        }
        // Determinism (§6g): rank desc, then date desc, then url. Unranked
        // relations (`linked_from`) fall through to date-then-url, newest
        // first — the order the citations already carry.
        scored.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.2.cmp(&a.2))
                .then_with(|| a.0.cmp(&b.0))
        });
        scored.truncate(rel.limit);
        scored.into_iter().map(|(u, s, _)| (u, s)).collect()
    }

    /// The candidate rows for a relation's `over`. A set/collection is a fixed
    /// list; a derived name is this row's own graph/tree neighbours (already
    /// in `names`).
    fn pool_rows(&self, rel: &Relation, names: &HashMap<String, Vec<String>>) -> Vec<&Row> {
        match &rel.pool {
            Pool::Set(name) => self
                .db
                .views
                .get(name)
                .map(|v| v.members.iter().filter_map(|k| self.db.rows.get(k)).collect())
                .unwrap_or_default(),
            Pool::Collection(name) => self
                .db
                .rows
                .iter()
                .filter(|r| &r.collection == name)
                .collect(),
            Pool::Derived(name) => names
                .get(name)
                .map(|urls| urls.iter().filter_map(|u| self.db.row_by_url(u)).collect())
                .unwrap_or_default(),
        }
    }

    /// Resolve a relation's label into the row's locale. A `Key` reads the
    /// string table (defaulting to the relation name); the others are literal.
    fn label(&self, label: &RelLabel, locale: &str, name: &str) -> String {
        match label {
            RelLabel::Key(k) => self.cfg.i18n.string(k, locale).to_string(),
            RelLabel::Text(t) => t.clone(),
            RelLabel::PerLocale(m) => m
                .get(locale)
                .or_else(|| m.get(&self.cfg.i18n.default))
                .cloned()
                .unwrap_or_else(|| name.to_string()),
        }
    }
}

/// The two-row environment as the filter language sees it (§6g). `self`/
/// `candidate` are the rows' URLs; `self.X`/`candidate.X` delegate to the
/// underlying row; a bare relation name is its finished list.
struct Pair<'a> {
    self_row: &'a Row,
    cand_row: &'a Row,
    names: &'a HashMap<String, Vec<String>>,
}

impl filter::Row for Pair<'_> {
    fn field(&self, name: &str) -> Value {
        match name {
            "self" => Value::Str(self.self_row.url.clone()),
            "candidate" => Value::Str(self.cand_row.url.clone()),
            _ => {
                if let Some(f) = name.strip_prefix("self.") {
                    self.self_row.field(f)
                } else if let Some(f) = name.strip_prefix("candidate.") {
                    self.cand_row.field(f)
                } else if let Some(list) = self.names.get(name) {
                    Value::List(list.clone())
                } else {
                    Value::Null
                }
            }
        }
    }
}

/// The score-function context: embedding cosine and year gap by URL. Search
/// similarity is unwired (no config needs it yet), so it stays the trait's
/// `None` default.
struct RelCtx<'a> {
    engine: &'a Engine<'a>,
}

impl Ctx for RelCtx<'_> {
    fn similarity(&self, a: &str, b: &str) -> Option<f64> {
        let (va, vb) = (self.engine.vec_by_url.get(a)?, self.engine.vec_by_url.get(b)?);
        // Vectors are stored normalized, so a dot product is the cosine.
        Some(va.iter().zip(vb.iter()).map(|(x, y)| (x * y) as f64).sum())
    }

    fn year_gap(&self, a: &str, b: &str) -> Option<f64> {
        use chrono::Datelike;
        let ra = self.engine.db.row_by_url(a)?;
        let rb = self.engine.db.row_by_url(b)?;
        let (da, db) = (ra.date?, rb.date?);
        Some((da.year() - db.year()).abs() as f64)
    }
}

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

/// Segment count of a pretty URL — `/a/b/` is depth 3, the parent of the
/// depth-4 `/a/b/c/`.
fn depth(url: &str) -> usize {
    url.matches('/').count()
}

/// The parent pretty-dir URL: `/a/b/` -> `/a/`, `/a/` -> `/`, `/` -> None.
fn url_parent(url: &str) -> Option<String> {
    let trimmed = url.strip_suffix('/')?;
    let cut = trimmed.rfind('/')?;
    Some(url[..=cut].to_string())
}

/// The derived names a collection's relations reference — through a `where`,
/// a `rank`, or an `over`. Only these get computed per row.
fn needed_derived(rels: &[Relation]) -> std::collections::HashSet<&'static str> {
    let mut needed = std::collections::HashSet::new();
    for rel in rels {
        let mut fields = rel.filter.referenced_fields();
        if let Some(rk) = &rel.rank {
            fields.extend(rk.referenced_fields());
        }
        if let Pool::Derived(n) = &rel.pool {
            fields.push(n.clone());
        }
        for f in fields {
            if let Some(d) = grackle_model::DERIVED_RELATIONS
                .iter()
                .copied()
                .find(|d| *d == f)
            {
                needed.insert(d);
            }
        }
    }
    needed
}

/// The canonical render order (§6g): the four defaults in reading order, then
/// any site-defined relation by name. Distinct from evaluation order, which is
/// dependency-driven.
fn render_rank(name: &str) -> (u8, &str) {
    let primary = match name {
        "earlier" => 0,
        "later" => 1,
        "related" => 2,
        "linked_from" => 3,
        _ => 4,
    };
    (primary, name)
}

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
        // Only the derived names this collection's relations actually read —
        // `linked_from` is a hashmap hit, but the tree family is an O(rows)
        // walk, so computing an unreferenced one per row is pure waste.
        let needed = needed_derived(rels);
        // The relation NAMES this row resolves — derived names first, then
        // each declared list as it is decided (dependency order guarantees a
        // reference is already present).
        let mut names: HashMap<String, Vec<String>> = self.derived_names(row, &needed);
        let mut groups = Vec::new();
        for rel in rels {
            if let Some(scope) = &rel.scope {
                if !scope.is_match(row.rel.to_string_lossy().as_ref()) {
                    continue;
                }
            }
            let members = self.evaluate(row, rel, &names, &ctx);
            names.insert(
                rel.name.clone(),
                members.iter().map(|(u, _)| u.clone()).collect(),
            );
            if members.is_empty() {
                continue;
            }
            let items = members
                .into_iter()
                .filter_map(|(url, _)| {
                    self.db
                        .row_by_url(&url)
                        .map(|r| (r.title.clone().unwrap_or_default(), r.url.clone(), r.date))
                })
                .collect();
            groups.push(Group {
                name: rel.name.clone(),
                label: self.label(&rel.label, row.locale(), &rel.name),
                items,
            });
        }
        // Evaluation ran in dependency order (`related` reads `earlier`), but
        // that is not reading order — render in a canonical one instead, the
        // way `parts.toml` fixes a kind's part order. Defaults read
        // chronological-neighbours-first; site relations follow, by name.
        groups.sort_by(|a, b| render_rank(&a.name).cmp(&render_rank(&b.name)));
        groups
    }

    /// The engine-provided names a row's relations read (§6g graph + path
    /// families), computed lazily — only the `needed` ones.
    fn derived_names(
        &self,
        row: &Row,
        needed: &std::collections::HashSet<&'static str>,
    ) -> HashMap<String, Vec<String>> {
        let mut m = HashMap::new();
        // The link graph, already the citation view (splice excluded) — cheap
        // hashmap hits.
        if needed.contains("linked_from") {
            m.insert(
                "linked_from".to_string(),
                self.backlinks
                    .get(&row.url)
                    .map(|v| v.iter().map(|(_, u, _)| u.clone()).collect())
                    .unwrap_or_default(),
            );
        }
        if needed.contains("links_to") {
            m.insert(
                "links_to".to_string(),
                self.links_to.get(&row.url).cloned().unwrap_or_default(),
            );
        }
        // The tree family. `ancestors`/`parent` share the breadcrumb walk;
        // `children`/`siblings`/`descendants` share an O(rows) URL-nesting
        // scan — each behind its own guard.
        if needed.contains("ancestors") || needed.contains("parent") {
            let anc: Vec<String> = crate::trails::ancestors(self.cfg, self.db, &row.url)
                .into_iter()
                .map(|(u, _)| u)
                .collect();
            if needed.contains("parent") {
                m.insert(
                    "parent".to_string(),
                    anc.last().cloned().into_iter().collect(),
                );
            }
            m.insert("ancestors".to_string(), anc);
        }
        if needed.contains("children")
            || needed.contains("siblings")
            || needed.contains("descendants")
        {
            let (children, siblings, descendants) = self.tree_family(row);
            m.insert("children".to_string(), children);
            m.insert("siblings".to_string(), siblings);
            m.insert("descendants".to_string(), descendants);
        }
        m
    }

    /// `children`/`siblings`/`descendants` from URL nesting: a pretty-dir URL
    /// `/a/b/` is the parent of `/a/b/c/`. Derived from the finished route set
    /// rather than a stored tree — nothing on grack.com declares a relation
    /// over these yet, but §6g names them, so they exist. Pure URL math, so a
    /// sibling is another child of `url_parent`, not (the old bug) the parent
    /// itself.
    fn tree_family(&self, row: &Row) -> (Vec<String>, Vec<String>, Vec<String>) {
        let self_url = row.url.as_str();
        let parent = url_parent(self_url);
        let mut children = Vec::new();
        let mut siblings = Vec::new();
        let mut descendants = Vec::new();
        for r in self.db.rows.iter() {
            if !r.rendered || r.url == row.url || r.locale() != row.locale() {
                continue;
            }
            if self_url.ends_with('/') && r.url.starts_with(self_url) {
                descendants.push(r.url.clone());
                if depth(&r.url) == depth(self_url) + 1 {
                    children.push(r.url.clone());
                }
            }
            if let Some(p) = &parent {
                if r.url.starts_with(p.as_str()) && depth(&r.url) == depth(p) + 1 {
                    siblings.push(r.url.clone());
                }
            }
        }
        (children, siblings, descendants)
    }

    /// The candidate as `self` should see it (§6f): a pool is default-locale,
    /// so a French page's neighbours are the French *variants* of what the
    /// pool holds — pivoted through `by_logical`, dropped where no variant
    /// exists. Without this a translated page's every relation is a desert.
    fn localize<'r>(&'r self, cand: &'r Row, locale: &str) -> Option<&'r Row> {
        if cand.locale() == locale {
            return Some(cand);
        }
        self.db
            .by_logical
            .get(&cand.logical)?
            .iter()
            .filter_map(|k| self.db.rows.get(k))
            .find(|r| r.locale() == locale)
    }

    /// Walk one relation's candidates: pool → localized to self → self-excluded
    /// → `where` → `rank` (+ `min_rank`) → sort → `limit`. Returns
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
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for cand in pool {
            // Pivot into self's locale; two pool members can pivot to one
            // variant, so dedup.
            let Some(cand) = self.localize(cand, row.locale()) else {
                continue;
            };
            // Self is never a candidate — a mechanism rule, not a per-site
            // `where` clause (§6g).
            if cand.url == row.url || !seen.insert(cand.url.as_str()) {
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
                .map(|v| {
                    v.members
                        .iter()
                        .filter_map(|k| self.db.rows.get(k))
                        .collect()
                })
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

/// The score-function context: embedding cosine and year gap by URL.
struct RelCtx<'a> {
    engine: &'a Engine<'a>,
}

impl Ctx for RelCtx<'_> {
    fn similarity(&self, a: &str, b: &str) -> Option<f64> {
        let (va, vb) = (
            self.engine.vec_by_url.get(a)?,
            self.engine.vec_by_url.get(b)?,
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use grackle_model::{Row, SiteDb};

    fn tree_row(url: &str) -> Row {
        let mut r = Row {
            rel: std::path::PathBuf::from(format!(
                "{}.md",
                url.trim_matches('/').replace('/', "-")
            )),
            url: url.to_string(),
            rendered: true,
            collection: "pages".into(),
            ..Row::default()
        };
        r.set_locale("en");
        r
    }

    fn cfg() -> Config {
        Config::from_toml(
            "root=\".\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nsource=\".\"\n",
        )
        .unwrap()
    }

    #[test]
    fn url_parent_and_depth_agree() {
        assert_eq!(url_parent("/a/b/"), Some("/a/".to_string()));
        assert_eq!(url_parent("/a/"), Some("/".to_string()));
        assert_eq!(url_parent("/"), None);
        assert_eq!(depth("/a/b/"), 3);
        assert_eq!(depth("/a/"), 2);
    }

    #[test]
    fn tree_family_is_children_siblings_descendants_not_the_parent() {
        let db = SiteDb::seed(
            vec![
                tree_row("/guide/"),
                tree_row("/guide/a/"),
                tree_row("/guide/b/"),
                tree_row("/guide/a/x/"),
            ],
            false,
        );
        let (cfg, links, backlinks) = (cfg(), HashMap::new(), HashMap::new());
        let eng = Engine::new(&cfg, &db, &[], &links, &backlinks);
        let me = db.row_by_url("/guide/a/").unwrap();
        let (children, siblings, descendants) = eng.tree_family(me);
        assert_eq!(children, ["/guide/a/x/"]);
        // The old off-by-one returned the parent /guide/ here.
        assert_eq!(siblings, ["/guide/b/"]);
        assert_eq!(descendants, ["/guide/a/x/"]);
    }

    #[test]
    fn localize_pivots_a_candidate_into_selfs_locale() {
        let mut en = tree_row("/post/");
        en.logical = "post".into();
        let mut fr = tree_row("/fr/post/");
        fr.set_locale("fr");
        fr.logical = "post".into();
        let mut lone = tree_row("/lone/");
        lone.logical = "lone".into();
        let mut db = SiteDb::seed(vec![en, fr, lone], false);
        let paired: Vec<_> = db
            .rows
            .iter()
            .filter(|r| r.logical == "post")
            .map(|r| r.key.clone())
            .collect();
        db.by_logical.insert("post".into(), paired);
        let (cfg, links, backlinks) = (cfg(), HashMap::new(), HashMap::new());
        let eng = Engine::new(&cfg, &db, &[], &links, &backlinks);

        let en_post = db.row_by_url("/post/").unwrap();
        assert_eq!(eng.localize(en_post, "fr").unwrap().url, "/fr/post/");
        assert_eq!(eng.localize(en_post, "en").unwrap().url, "/post/");
        // A row with no variant in the target locale drops out.
        let lone = db.row_by_url("/lone/").unwrap();
        assert!(eng.localize(lone, "fr").is_none());
    }
}

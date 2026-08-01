//! Compile collection relations at load time (DESIGN.md §6g). Evaluation deferred to build.

use anyhow::{bail, Context, Result};
use std::collections::{BTreeMap, HashMap};

use grackle_db::filter::{self, Filter, Rank};
use grackle_model::{two_row_schema, Pool, RelLabel, Relation, SiteDb, DERIVED_RELATIONS};

use crate::config::{Config, LocalizedStr, RelationCfg};
use crate::schema::Schemas;

pub(crate) fn build_relations(cfg: &Config, db: &mut SiteDb, schemas: &Schemas) -> Result<()> {
    let mut out: BTreeMap<String, Vec<Relation>> = BTreeMap::new();
    for (cname, c) in &cfg.collections {
        if c.relations.is_empty() {
            continue;
        }
        let specs: &BTreeMap<String, RelationCfg> = &c.relations;

        let names: Vec<String> = specs.keys().cloned().collect();
        let base = schemas.row_filter_schema();
        let schema = two_row_schema(&base, &names);

        let mut compiled: HashMap<String, Relation> = HashMap::new();
        for (name, rc) in specs {
            let who = format!("collection {cname}: relation {name}");
            let rel = compile_one(cfg, db, cname, name, rc, &schema).with_context(|| who)?;
            compiled.insert(name.clone(), rel);
        }

        out.insert(cname.clone(), order_by_dependency(cname, compiled, &names)?);
    }
    db.relations = out;
    Ok(())
}

fn compile_one(
    cfg: &Config,
    db: &SiteDb,
    cname: &str,
    name: &str,
    rc: &RelationCfg,
    schema: &filter::Schema,
) -> Result<Relation> {
    let pool = resolve_pool(cfg, db, cname, rc.from.as_deref())?;
    let where_src = match rc.filter.as_deref() {
        Some(f) => f.to_string(),
        None => "*".to_string(),
    };
    let filter =
        Filter::parse(&where_src, schema).with_context(|| format!("where {where_src:?}"))?;
    let rank = match rc.rank.as_deref() {
        Some(src) => Some(Rank::parse(src, schema).with_context(|| format!("rank {src:?}"))?),
        None => None,
    };
    let scope = match rc.scope.as_deref() {
        Some(g) => Some(
            globset::Glob::new(g)
                .with_context(|| format!("scope {g:?}"))?
                .compile_matcher(),
        ),
        None => None,
    };
    Ok(Relation {
        name: name.to_string(),
        pool,
        scope,
        filter,
        rank,
        min_rank: rc.min_rank,
        limit: rc.limit.unwrap_or(usize::MAX),
        label: resolve_label(rc.label.as_ref(), name),
    })
}

/// Absent `from` means the collection itself; filtered pools come from explicit set names.
fn resolve_pool(cfg: &Config, db: &SiteDb, cname: &str, from: Option<&str>) -> Result<Pool> {
    let Some(name) = from else {
        return Ok(Pool::Collection(cname.to_string()));
    };
    if DERIVED_RELATIONS.contains(&name) {
        return Ok(Pool::Derived(name.to_string()));
    }
    if db.views.contains_key(name) {
        return Ok(Pool::Set(name.to_string()));
    }
    if cfg.collections.contains_key(name) {
        return Ok(Pool::Collection(name.to_string()));
    }
    let mut sets: Vec<&str> = db.views.keys().map(String::as_str).collect();
    sets.sort_unstable();
    bail!(
        "from = {name:?} on collection {cname} names nothing: expected a set \
         ({}), a collection, or a derived relation ({})",
        sets.join(", "),
        DERIVED_RELATIONS.join(", "),
    );
}

/// Absent label = `@NAME` (the relation's own name as i18n key).
fn resolve_label(label: Option<&LocalizedStr>, name: &str) -> RelLabel {
    match label {
        None => RelLabel::Key(name.to_string()),
        Some(l) => match l.reference() {
            Some(key) => RelLabel::Key(key.to_string()),
            None => match l {
                LocalizedStr::One(s) => {
                    // `@@literal` escapes to a literal leading `@`.
                    RelLabel::Text(s.strip_prefix('@').unwrap_or(s).to_string())
                }
                LocalizedStr::PerMember(m) => RelLabel::PerMember(m.clone()),
            },
        },
    }
}

fn order_by_dependency(
    cname: &str,
    mut compiled: HashMap<String, Relation>,
    names: &[String],
) -> Result<Vec<Relation>> {
    let name_set: std::collections::HashSet<&str> = names.iter().map(String::as_str).collect();
    let deps_of = |r: &Relation| -> Vec<String> {
        let mut fields = r.filter.referenced_fields();
        if let Some(rk) = &r.rank {
            fields.extend(rk.referenced_fields());
        }
        if let Pool::Set(p) | Pool::Collection(p) = &r.pool {
            fields.push(p.clone());
        }
        fields
            .into_iter()
            .filter(|f| f != &r.name && name_set.contains(f.as_str()))
            .collect()
    };

    let mut initial: Vec<String> = Vec::new();
    for d in ["earlier", "later", "related", "linked_from"] {
        if compiled.contains_key(d) {
            initial.push(d.to_string());
        }
    }
    for n in names {
        if !initial.contains(n) {
            initial.push(n.clone());
        }
    }

    let mut emitted: Vec<String> = Vec::new();
    while emitted.len() < initial.len() {
        let next = initial.iter().find(|n| {
            !emitted.contains(*n) && deps_of(&compiled[*n]).iter().all(|d| emitted.contains(d))
        });
        match next {
            Some(n) => emitted.push(n.clone()),
            None => {
                let stuck: Vec<&str> = initial
                    .iter()
                    .filter(|n| !emitted.contains(*n))
                    .map(String::as_str)
                    .collect();
                bail!(
                    "collection {cname}: relations reference each other in a \
                     cycle: {}",
                    stuck.join(", ")
                );
            }
        }
    }
    Ok(emitted
        .into_iter()
        .map(|n| compiled.remove(&n).unwrap())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use grackle_model::{row_schema, SiteDb};

    /// The base.toml posts-collection neighbour defaults, for tests that need
    /// them without merging the full base config.
    ///
    /// Relation tables MUST come before `[sets.…]`: TOML attaches
    /// `[collections.relations.*]` to the last `[[collections]]` only while
    /// that array entry is still open.
    const POSTS_DEFAULTS: &str = r#"
[collections.relations.earlier]
from = "published"
where = "candidate.date.year * 10000 + candidate.date.month * 100 + candidate.date.day < self.date.year * 10000 + self.date.month * 100 + self.date.day"
rank = "candidate.date.year * 10000 + candidate.date.month * 100 + candidate.date.day"
limit = 1

[collections.relations.later]
from = "published"
where = "candidate.date.year * 10000 + candidate.date.month * 100 + candidate.date.day > self.date.year * 10000 + self.date.month * 100 + self.date.day"
rank = "-(candidate.date.year * 10000 + candidate.date.month * 100 + candidate.date.day)"
limit = 1

[collections.relations.related]
from = "published"
where = "!(candidate in earlier) && !(candidate in later) && !(candidate in links_to)"
rank = "embedding_similarity(self, candidate)"
limit = 4

[collections.relations.linked_from]
from = "linked_from"
where = "!(candidate in ancestors)"

[sets.published]
from = "posts"
where = "!draft && !hidden"
order_by = "-date"
"#;

    /// Compile relations for a single posts collection with `extra` config
    /// appended (relation blocks, sets). Runs `build_views` first so a declared
    /// `[sets.published]` resolves as a pool.
    fn compile(extra: &str) -> Result<Vec<Relation>> {
        let toml = format!(
            "root=\".\"\nextends=\"none\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [schema]\ndraft={{type=\"bool\"}}\nhidden={{type=\"bool\"}}\ndate={{type=\"date\"}}\n\
             [[collections]]\nname=\"posts\"\nsource=\"_posts\"\n\
             file=[\"{{date.year}}-{{date.month}}-{{date.day}}-{{slug}}\"]\n{extra}"
        );
        let cfg = Config::from_toml(&toml)?;
        let mut db = SiteDb::seed(vec![], true);
        // What `load` does: the config axes go in before anything resolves.
        let mut schemas = Schemas::new(row_schema());
        schemas.set_site(cfg.schema.fields.clone(), "[schema]")?;
        crate::views::build_views(&cfg, &mut db, &schemas)?;
        build_relations(&cfg, &mut db, &schemas)?;
        Ok(db.relations.remove("posts").unwrap_or_default())
    }

    fn relation<'a>(rels: &'a [Relation], name: &str) -> &'a Relation {
        rels.iter().find(|r| r.name == name).unwrap()
    }

    fn names(rels: &[Relation]) -> Vec<&str> {
        rels.iter().map(|r| r.name.as_str()).collect()
    }

    /// The full error chain (`{:#}`), so a wrapped root cause — a bad field
    /// under a `collection … relation …` context — is visible.
    fn err(extra: &str) -> String {
        format!("{:#}", compile(extra).unwrap_err())
    }

    /// `extends = "none"` with no relation blocks is truly empty — the engine
    /// mints nothing (§4d, same as the flag family).
    #[test]
    fn extends_none_mints_no_relations() {
        let rels = compile("").unwrap();
        assert!(rels.is_empty(), "expected no relations, got {rels:?}");
    }

    /// The base config is where the four live now: an empty site that inherits
    /// it gets them on posts (and `linked_from` on the tree), with nothing
    /// compiled in Rust.
    #[test]
    fn inheriting_base_ships_posts_and_tree_relations() {
        let cfg = Config::from_toml(
            "root=\".\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n",
        )
        .expect("empty site inherits base");
        let posts = &cfg.collections["posts"].relations;
        for want in ["earlier", "later", "related", "linked_from"] {
            assert!(posts.contains_key(want), "posts missing {want}: {posts:?}");
        }
        assert_eq!(
            posts["earlier"].from.as_deref(),
            Some("published"),
            "base writes from explicitly"
        );
        let tree = &cfg.collections["entries"].relations;
        assert!(
            tree.contains_key("linked_from") && tree.len() == 1,
            "tree gets only linked_from: {tree:?}"
        );
        assert!(
            cfg.collections["objects"].relations.is_empty(),
            "objects stay relation-free"
        );
    }

    #[test]
    fn base_defaults_are_present_and_related_follows_its_neighbours() {
        let rels = compile(POSTS_DEFAULTS).unwrap();
        let ns = names(&rels);
        for want in ["earlier", "later", "related", "linked_from"] {
            assert!(ns.contains(&want), "missing default {want}: {ns:?}");
        }
        // `related` reads `earlier`/`later`, so it must evaluate after them.
        let pos = |n: &str| ns.iter().position(|x| *x == n).unwrap();
        assert!(pos("related") > pos("earlier"), "{ns:?}");
        assert!(pos("related") > pos("later"), "{ns:?}");
    }

    #[test]
    fn a_reference_cycle_is_a_load_error() {
        let e = err(
            "[collections.relations.a]\nfrom=\"posts\"\nwhere=\"!(candidate in b)\"\n\
             [collections.relations.b]\nfrom=\"posts\"\nwhere=\"!(candidate in a)\"\n",
        );
        assert!(e.contains("cycle"), "{e}");
        assert!(e.contains('a') && e.contains('b'), "{e}");
    }

    #[test]
    fn an_unknown_two_row_field_is_caught_at_load() {
        let e = err("[collections.relations.x]\nfrom=\"posts\"\nwhere=\"candidate.nope\"\n");
        assert!(
            e.contains("unknown field") && e.contains("candidate.nope"),
            "{e}"
        );
    }

    #[test]
    fn a_non_numeric_rank_is_caught_at_load() {
        let e = err("[collections.relations.x]\nfrom=\"posts\"\nrank=\"candidate.title\"\n");
        assert!(e.contains("must be a number"), "{e}");
    }

    #[test]
    fn a_site_may_declare_related_differently_beside_the_other_defaults() {
        // Same shape as a site that inherits base and overrides `related` by
        // name: the other three stay, the one named is the site's.
        let extra = r#"
[collections.relations.earlier]
from = "published"
where = "candidate.date.year * 10000 + candidate.date.month * 100 + candidate.date.day < self.date.year * 10000 + self.date.month * 100 + self.date.day"
rank = "candidate.date.year * 10000 + candidate.date.month * 100 + candidate.date.day"
limit = 1

[collections.relations.later]
from = "published"
where = "candidate.date.year * 10000 + candidate.date.month * 100 + candidate.date.day > self.date.year * 10000 + self.date.month * 100 + self.date.day"
rank = "-(candidate.date.year * 10000 + candidate.date.month * 100 + candidate.date.day)"
limit = 1

[collections.relations.related]
from = "posts"
limit = 2

[collections.relations.linked_from]
from = "linked_from"
where = "!(candidate in ancestors)"

[sets.published]
from = "posts"
where = "!draft && !hidden"
"#;
        let rels = compile(extra).unwrap();
        let ns = names(&rels);
        assert!(
            ns.contains(&"earlier") && ns.contains(&"linked_from"),
            "{ns:?}"
        );
        assert_eq!(relation(&rels, "related").limit, 2);
    }

    /// The engine composes NO predicate of its own. It used to AND
    /// `!candidate.draft && !candidate.hidden` onto a defaulted pool, which
    /// meant engine code naming two fields it had no business knowing; the
    /// guarantee lives in the base config's `published` set now (§4d).
    ///
    /// Mutation-checked by restoring the injection, which makes this fail and
    /// `a_relation_over_a_set_reads_only_what_it_declared` fail with it.
    #[test]
    fn the_engine_ands_nothing_onto_a_declared_predicate() {
        let refs = relation(&compile(POSTS_DEFAULTS).unwrap(), "earlier")
            .filter
            .referenced_fields();
        assert_eq!(
            refs,
            vec![
                "candidate.date.year".to_string(),
                "candidate.date.month".to_string(),
                "candidate.date.day".to_string(),
                "self.date.year".to_string(),
                "self.date.month".to_string(),
                "self.date.day".to_string(),
            ],
            "the default `earlier` compares date-field ordinals and nothing else"
        );
    }

    /// The same, one level out: whatever pool a relation ranges over, its
    /// `where` is the one the config wrote.
    #[test]
    fn a_relation_over_a_set_reads_only_what_it_declared() {
        let rels = compile(
            "[collections.relations.x]\nwhere=\"candidate.date.year == 2026\"\n\
             [sets.published]\nfrom=\"posts\"\nwhere=\"!draft && !hidden\"\n",
        )
        .unwrap();
        let refs = relation(&rels, "x").filter.referenced_fields();
        assert_eq!(refs, vec!["candidate.date.year".to_string()]);
    }

    /// Absent `from` means the collection itself — never a hard-coded set
    /// name. The base writes `from = "published"` explicitly.
    #[test]
    fn absent_from_is_the_collection_itself() {
        let rels = compile(
            "[collections.relations.x]\nwhere=\"candidate.date.year == 2026\"\n",
        )
        .unwrap();
        assert!(
            matches!(
                &relation(&rels, "x").pool,
                Pool::Collection(n) if n == "posts"
            ),
            "{:?}",
            relation(&rels, "x").pool
        );
    }

    /// With an explicit `from = "published"`, the pool is that set, and the
    /// filtering it does is its own — `earlier` still reads only the date.
    #[test]
    fn a_published_set_needs_no_fallback_filter() {
        let refs = relation(&compile(POSTS_DEFAULTS).unwrap(), "earlier")
            .filter
            .referenced_fields();
        assert!(!refs.iter().any(|f| f == "candidate.draft"), "{refs:?}");
        assert!(
            matches!(
                &relation(&compile(POSTS_DEFAULTS).unwrap(), "earlier").pool,
                Pool::Set(n) if n == "published"
            )
        );
    }
}

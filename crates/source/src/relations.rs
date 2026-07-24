//! Compiling declared relations (DESIGN.md §6g).
//!
//! Each collection's neighbour lists — `earlier`, `later`, `related`,
//! `linked_from`, and whatever a site invents — are small queries over a
//! **two-row environment** (`self` and `candidate`). This module turns the
//! config into type-checked, dependency-ordered [`Relation`]s on the db; the
//! engine (grackle crate) evaluates them per row at build.
//!
//! What happens here, and why here: parsing and type-checking are load-time
//! (§5f), so a `where` naming a field no row has, or a `rank` that is not a
//! number, or a reference cycle, is a startup error naming the relation —
//! never a render surprise. Evaluation is deferred, because it needs the
//! embedding vectors and the link graph, which live at build.

use anyhow::{bail, Context, Result};
use std::collections::{BTreeMap, HashMap};

use grackle_db::filter::{self, Filter, Rank};
use grackle_model::{
    object_schema, row_schema, two_row_schema, Kind, Pool, RelLabel, Relation, SiteDb,
    DERIVED_RELATIONS,
};

use crate::config::{Config, LocalizedStr, RelationCfg};
use crate::schema::Schemas;

/// Compile every collection's relations onto `db.relations`, in dependency
/// order. Runs after `build_views`, so the sets a relation ranges `over` are
/// already resolved.
pub(crate) fn build_relations(cfg: &Config, db: &mut SiteDb, schemas: &Schemas) -> Result<()> {
    let mut out: BTreeMap<String, Vec<Relation>> = BTreeMap::new();
    for (cname, c) in &cfg.collections {
        // Defaults for the kind, then the site's declarations layered on top —
        // per NAME, so changing `related` leaves `earlier`/`later` alone
        // (§6g). A declared block replaces its default wholesale.
        let mut specs: BTreeMap<String, RelationCfg> = default_relations(c.kind);
        for (name, rc) in &c.relations {
            specs.insert(name.clone(), rc.clone());
        }
        if specs.is_empty() {
            continue;
        }

        // The two-row schema every expression here type-checks against: the
        // base fields under `self.`/`candidate.`, plus every relation name of
        // THIS collection as a list. Declared `.schema.toml` fields join the
        // base so `self.course` resolves (§6g same_course).
        let names: Vec<String> = specs.keys().cloned().collect();
        let base = base_schema(c.kind, schemas);
        let schema = two_row_schema(&base, &names);

        let mut compiled: HashMap<String, Relation> = HashMap::new();
        for (name, rc) in &specs {
            let who = format!("collection {cname}: relation {name}");
            let rel = compile_one(cfg, db, cname, name, rc, &schema).with_context(|| who)?;
            compiled.insert(name.clone(), rel);
        }

        out.insert(cname.clone(), order_by_dependency(cname, compiled, &names)?);
    }
    db.relations = out;
    Ok(())
}

/// The base (single-row) schema a collection's `self`/`candidate` fields come
/// from: the object vocabulary for binaries, otherwise the row schema plus
/// every declared `.schema.toml` field.
fn base_schema(kind: Kind, schemas: &Schemas) -> filter::Schema {
    if kind == Kind::Objects {
        return object_schema();
    }
    let mut s = row_schema();
    for (name, ty) in schemas.declared() {
        // A declaration never shadows a built-in (schema.rs enforces that at
        // load), so plain insert is fine. Interned, not leaked, so `serve`
        // reloads do not accumulate keys.
        s.insert(grackle_model::intern(name.to_string()), ty.filter_type());
    }
    s
}

fn compile_one(
    cfg: &Config,
    db: &SiteDb,
    cname: &str,
    name: &str,
    rc: &RelationCfg,
    schema: &filter::Schema,
) -> Result<Relation> {
    let pool = resolve_pool(cfg, db, cname, rc.over.as_deref())?;
    // A defaulted `over` that fell back to the bare collection (no `published`
    // set) must still drop drafts and hidden rows — otherwise a dated draft
    // becomes somebody's Later post (q51's exact hole). An EXPLICIT `over` is
    // taken as written; only the engine defaults' fallback carries this rule.
    let fallback = rc.over.is_none() && matches!(&pool, Pool::Collection(_));
    let where_src = match (rc.filter.as_deref(), fallback) {
        (Some(f), true) => format!("({f}) && !candidate.draft && !candidate.hidden"),
        (None, true) => "!candidate.draft && !candidate.hidden".to_string(),
        (Some(f), false) => f.to_string(),
        (None, false) => "*".to_string(),
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
                .with_context(|| format!("match {g:?}"))?
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
        // No `limit` means the whole list — `linked_from` shows every citation;
        // `earlier`/`later`/`related` set their windows.
        limit: rc.limit.unwrap_or(usize::MAX),
        label: resolve_label(rc.label.as_ref(), name),
    })
}

/// Resolve `over` to a candidate pool. A derived name is row-relative; a set
/// or collection is a fixed list.
///
/// An explicit `over` must resolve or it is a load error. An ABSENT one is the
/// engine defaults' pool: the `published` set — the shape every site's
/// published corpus has — falling back to the collection itself, then filtered
/// `!draft && !hidden` by the caller, when a site declares no such set (§6g:
/// a `published` set is not mandatory just to get neighbours).
fn resolve_pool(cfg: &Config, db: &SiteDb, cname: &str, over: Option<&str>) -> Result<Pool> {
    let Some(name) = over else {
        return Ok(if db.views.contains_key("published") {
            Pool::Set("published".to_string())
        } else {
            Pool::Collection(cname.to_string())
        });
    };
    if DERIVED_RELATIONS.contains(&name) {
        return Ok(Pool::Derived(name.to_string()));
    }
    // A set is a routeless view resolved to one member list.
    if db.views.contains_key(name) {
        return Ok(Pool::Set(name.to_string()));
    }
    if cfg.collections.contains_key(name) {
        return Ok(Pool::Collection(name.to_string()));
    }
    let mut sets: Vec<&str> = db.views.keys().map(String::as_str).collect();
    sets.sort_unstable();
    bail!(
        "over = {name:?} on collection {cname} names nothing: expected a set \
         ({}), a collection, or a derived relation ({})",
        sets.join(", "),
        DERIVED_RELATIONS.join(", "),
    );
}

/// A relation's heading (§6g). Absent = `@NAME`, the string key equal to the
/// relation's own name — so a site that declares `same_course` gets a
/// `same_course` label key for free and only writes strings for the locales
/// it cares about.
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
                LocalizedStr::PerLocale(m) => RelLabel::PerLocale(m.clone()),
            },
        },
    }
}

/// The four engine defaults (§6g). Posts get the full set; a tree or object
/// collection gets only `linked_from` — `earlier`/`later`/`related` are
/// date-and-similarity ideas a page has no answer for, and running them would
/// be an empty group at O(n²) cost. A declared relation on any collection is
/// added regardless.
fn default_relations(kind: Kind) -> BTreeMap<String, RelationCfg> {
    let mut m = BTreeMap::new();
    let rel = |over: Option<&str>, filter: Option<&str>, rank: Option<&str>, limit: Option<usize>| {
        RelationCfg {
            over: over.map(str::to_string),
            filter: filter.map(str::to_string),
            scope: None,
            rank: rank.map(str::to_string),
            min_rank: None,
            limit,
            label: None,
        }
    };
    // Every collection can point at what links to it.
    m.insert(
        "linked_from".to_string(),
        rel(Some("linked_from"), Some("!(candidate in ancestors)"), None, None),
    );
    if kind == Kind::Posts {
        // The nearest neighbour on each side. `date` is an ISO string, which
        // compares chronologically in `where` — but a rank must be a NUMBER,
        // so the ordinal is built from the y/m/d columns: bigger = later, so
        // `earlier` takes the max below self and `later` negates to take the
        // min above. An undated candidate yields Null and drops out. Verbose,
        // but it is an engine default no site ever types.
        //
        // Known latent: `<` is strict and the ordinal is day-granular, so two
        // posts on ONE day are neither's neighbour (and tie in rank). Measured
        // zero same-day pairs in the corpus; the day the first appears, add a
        // slug tiebreak to the ordinal. The old position-based walk handled it
        // by luck of sort order, not by rule.
        const DATE_ORD: &str = "candidate.year * 10000 + candidate.month * 100 + candidate.day";
        m.insert(
            "earlier".to_string(),
            rel(None, Some("candidate.date < self.date"), Some(DATE_ORD), Some(1)),
        );
        m.insert(
            "later".to_string(),
            rel(None, Some("candidate.date > self.date"), Some(&format!("-({DATE_ORD})")), Some(1)),
        );
        m.insert(
            "related".to_string(),
            rel(
                None,
                // Not the neighbours already shown, not a page it links to
                // (defect 1). `links_to` is the derived forward-link name.
                Some("!(candidate in earlier) && !(candidate in later) && !(candidate in links_to)"),
                Some("embedding_similarity(self, candidate)"),
                Some(4),
            ),
        );
    }
    m
}

/// Order relations so each evaluates after the ones it names. `related`
/// references `earlier`/`later`, so it must follow them; a reference cycle is
/// a load error, never a render-time surprise. The initial order is the
/// reading order — the defaults first, then extras alphabetically — and the
/// sort is stable within it.
fn order_by_dependency(
    cname: &str,
    mut compiled: HashMap<String, Relation>,
    names: &[String],
) -> Result<Vec<Relation>> {
    // Deps of a relation = the OTHER relation names its where/rank read, plus
    // its pool if the pool names one. Derived names are always available, so
    // they impose no ordering (they are not in `names`).
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

    // A deterministic initial order: the reading order of the defaults, then
    // any extras in the sorted order `names` already carries.
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

    // Stable topological sort: repeatedly emit the first not-yet-emitted node
    // whose deps are all emitted. None emittable with nodes left ⇒ a cycle.
    let mut emitted: Vec<String> = Vec::new();
    while emitted.len() < initial.len() {
        let next = initial.iter().find(|n| {
            !emitted.contains(*n)
                && deps_of(&compiled[*n]).iter().all(|d| emitted.contains(d))
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
    Ok(emitted.into_iter().map(|n| compiled.remove(&n).unwrap()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use grackle_model::{row_schema, SiteDb};

    /// Compile relations for a single posts collection with `extra` config
    /// appended (relation blocks, sets). Runs `build_views` first so a declared
    /// `[sets.published]` resolves as a pool.
    fn compile(extra: &str) -> Result<Vec<Relation>> {
        let toml = format!(
            "root=\".\"\n[site]\nurl=\"u\"\ntitle=\"t\"\nauthor=\"a\"\n\
             [[collections]]\nname=\"posts\"\nkind=\"posts\"\nsource=\"_posts\"\n\
             filename_formats=[\"{{year}}-{{month}}-{{day}}-{{slug}}\"]\n{extra}"
        );
        let cfg = Config::from_toml(&toml)?;
        let mut db = SiteDb::seed(vec![], true);
        let schemas = Schemas::new(row_schema());
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

    #[test]
    fn defaults_are_present_and_related_follows_its_neighbours() {
        let rels = compile("").unwrap();
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
            "[collections.relations.a]\nover=\"posts\"\nwhere=\"!(candidate in b)\"\n\
             [collections.relations.b]\nover=\"posts\"\nwhere=\"!(candidate in a)\"\n",
        );
        assert!(e.contains("cycle"), "{e}");
        assert!(e.contains('a') && e.contains('b'), "{e}");
    }

    #[test]
    fn an_unknown_two_row_field_is_caught_at_load() {
        let e = err("[collections.relations.x]\nover=\"posts\"\nwhere=\"candidate.nope\"\n");
        assert!(e.contains("unknown field") && e.contains("candidate.nope"), "{e}");
    }

    #[test]
    fn a_non_numeric_rank_is_caught_at_load() {
        let e = err("[collections.relations.x]\nover=\"posts\"\nrank=\"candidate.title\"\n");
        assert!(e.contains("must be a number"), "{e}");
    }

    #[test]
    fn a_declared_relation_overrides_its_default_by_name() {
        // Overriding `related` leaves the other three defaults in place.
        let rels =
            compile("[collections.relations.related]\nover=\"posts\"\nlimit=2\n").unwrap();
        let ns = names(&rels);
        assert!(ns.contains(&"earlier") && ns.contains(&"linked_from"), "{ns:?}");
        assert_eq!(relation(&rels, "related").limit, 2);
    }

    /// The guarantee the deleted `adjacency` test carried: with no `published`
    /// set the defaults fall back to the collection, so they must exclude
    /// drafts by rule (else a dated draft becomes somebody's Later post).
    #[test]
    fn the_fallback_pool_drops_drafts_by_rule() {
        let refs = relation(&compile("").unwrap(), "earlier").filter.referenced_fields();
        assert!(refs.iter().any(|f| f == "candidate.draft"), "{refs:?}");
        assert!(refs.iter().any(|f| f == "candidate.hidden"), "{refs:?}");
    }

    /// With a `published` set the pool is that set — already filtered — so the
    /// fallback rule does not fire and `earlier` reads only the date.
    #[test]
    fn a_published_set_needs_no_fallback_filter() {
        let rels =
            compile("[sets.published]\nfrom=\"posts\"\nwhere=\"!draft && !hidden\"\n").unwrap();
        let refs = relation(&rels, "earlier").filter.referenced_fields();
        assert!(!refs.iter().any(|f| f == "candidate.draft"), "{refs:?}");
    }
}

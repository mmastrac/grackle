//! Views become routes: resolving each declared query into row sets, group
//! partitions (subdivision, §5c) and materialized `Route`s. Split from the
//! table-building half of the database (`db.rs`); `SiteDb::load` calls in
//! here once the tables and row routes exist.

use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;

use crate::config::{Config, Kind, Query, View};
use crate::db::{object_schema, post_schema, route_schema, Post, Route, RouteKind, SiteDb, ViewRows};
use crate::filter;
use crate::route;

/// One group key a row contributes under a single `group_by` spec: the typed
/// sort component (years/months order numerically, tags lexically), the
/// display component (joined into `Route.key`), and the parameters the key
/// exposes to route/`title`/`crumb` templates.
#[derive(Clone, Debug)]
pub(crate) struct GroupKey {
    sort: SortKey,
    pub(crate) params: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum SortKey {
    Int(i64),
    Str(String),
}

impl SortKey {
    fn display(&self) -> String {
        match self {
            // Numeric parts zero-pad to two so `2022-3` reads `2022-03` —
            // years are 4 digits and unaffected.
            SortKey::Int(n) if *n < 10 => format!("0{n}"),
            SortKey::Int(n) => n.to_string(),
            SortKey::Str(s) => s.clone(),
        }
    }
}

/// The group keys a row holds under one spec. Empty means the row is absent
/// from this partition (an undated row under a date grouping).
fn group_keys(p: &Post, spec: &str) -> Result<Vec<GroupKey>> {
    use chrono::Datelike;
    Ok(match spec {
        "tags" => p
            .tags
            .iter()
            .map(|t| GroupKey {
                sort: SortKey::Str(t.clone()),
                params: vec![("key".into(), t.clone())],
            })
            .collect(),
        "date.year" => p
            .date
            .into_iter()
            .map(|d| GroupKey {
                sort: SortKey::Int(d.year() as i64),
                params: vec![("year".into(), d.year().to_string())],
            })
            .collect(),
        "date.month" => p
            .date
            .into_iter()
            .map(|d| GroupKey {
                sort: SortKey::Int(d.month() as i64),
                params: vec![
                    ("month".into(), d.month().to_string()),
                    ("month_name".into(), d.format("%B").to_string()),
                ],
            })
            .collect(),
        other => bail!("unsupported group_by {other:?} (have: tags, date.year, date.month)"),
    })
}

/// The composite keys a row belongs to under a subdivision chain — the
/// cartesian product across levels (`tags` can multi-key a row; date specs
/// contribute at most one each). Empty when the row is absent at any level.
pub(crate) fn key_combos(p: &Post, chain: &[String]) -> Result<Vec<Vec<GroupKey>>> {
    let mut combos: Vec<Vec<GroupKey>> = vec![Vec::new()];
    for spec in chain {
        let keys = group_keys(p, spec)?;
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        combos = combos
            .into_iter()
            .flat_map(|c| {
                keys.iter().map(move |k| {
                    let mut c2 = c.clone();
                    c2.push(k.clone());
                    c2
                })
            })
            .collect();
    }
    Ok(combos)
}

pub(crate) fn build_views(cfg: &Config, db: &mut SiteDb) -> Result<()> {
    for (name, v) in &cfg.views {
        // `over = "*"` views read the finished route set, so they run in a
        // second pass (see build_star_views). Views iterate in name order, so
        // running them inline made `sitemap` miss `tag_index` — 1544 not 1559.
        if v.over == "*" {
            continue;
        }
        // Both named queries (`published`) and embedded views (`latest`) still
        // have to resolve, so a typo in `over` is a startup error either way.
        let q = cfg.query(name)?;
        // Dispatch on the base collection's KIND, never its name. This
        // replaced the phase-1 `q.base != "blog"` gate the day the example
        // site (§7a) named its posts collection `notes` — the falsifier
        // doing its job.
        let Some(base) = cfg.collections.get(&q.base) else { continue };
        match base.kind {
            Kind::Posts => {} // the flow below
            Kind::Objects => {
                build_object_view(cfg, db, name, v, &q)?;
                continue;
            }
            Kind::Tree => {
                build_tree_view(cfg, db, name, v, &q)?;
                continue;
            }
        }
        // Parsed and type-checked once per view, not per row: a bad filter is a
        // startup error naming the view.
        let pred = match q.predicate() {
            Some(src) => filter::Filter::parse(&src, &post_schema())
                .with_context(|| format!("view {name}: filter {src:?}"))?,
            None => filter::Filter::always(),
        };
        let posts = &db.posts;
        let visible: Vec<usize> = posts
            .order
            .iter()
            .copied()
            .filter(|&i| pred.eval(&posts.rows[i]))
            .collect();

        // No route: one row set, and nowhere to hang it but the view itself.
        if !v.is_materialized() {
            let members: Vec<usize> =
                visible.into_iter().take(v.limit.unwrap_or(usize::MAX)).collect();
            db.views.insert(
                name.clone(),
                ViewRows {
                    layout: v.layout.clone(),
                    variant: v.variant.clone(),
                    rows: members.len(),
                    table: Kind::Posts,
                    members,
                },
            );
            continue;
        }

        // Grouped views, possibly a subdivision chain (§5c): a grouped view
        // `over` a grouped view refines the parent's partition, and the group
        // keys accumulate — GROUP BY year, month, expressed compositionally.
        // The chain is read from config alone, so processing order between
        // parent and child views doesn't matter.
        if v.group_by.is_some() {
            let tmpl = v
                .route
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("view {name} needs a route"))?;
            let chain = cfg.group_specs(name);
            // Composite key → (template params, members). BTreeMap on the
            // typed sort key keeps years/months numeric and tags lexical.
            let mut groups: BTreeMap<Vec<SortKey>, (Vec<(String, String)>, Vec<usize>)> =
                BTreeMap::new();
            for &i in &visible {
                for combo in key_combos(&posts.rows[i], &chain)? {
                    let sort: Vec<SortKey> = combo.iter().map(|k| k.sort.clone()).collect();
                    groups
                        .entry(sort)
                        .or_insert_with(|| {
                            let params =
                                combo.iter().flat_map(|k| k.params.clone()).collect();
                            (params, Vec::new())
                        })
                        .1
                        .push(i);
                }
            }
            for (sort, (params, members)) in groups {
                let url = route::render(tmpl, |k| route::param(&params, k))?;
                let key = sort
                    .iter()
                    .map(SortKey::display)
                    .collect::<Vec<_>>()
                    .join("-");
                db.routes.push(Route {
                    view: Some(name.clone()),
                    key: Some(key),
                    rows: Some(members.len()),
                    params,
                    members,
                    ..Route::new(url, RouteKind::View)
                });
            }
            continue;
        }

        match v.group_by.as_deref() {
            // Paginated list.
            None if v.paginate.is_some() => {
                let per = v.paginate.unwrap().max(1);
                let pages = visible.len().div_ceil(per);
                for n in 1..=pages {
                    let tmpl = if n == 1 {
                        v.routes.first()
                    } else {
                        v.routes.get(1).or_else(|| v.routes.first())
                    };
                    let Some(tmpl) = tmpl else { continue };
                    let url = route::render(tmpl, |k| match k {
                        "n" => Some(n.to_string()),
                        _ => None,
                    })?;
                    let members: Vec<usize> =
                        visible.iter().copied().skip(per * (n - 1)).take(per).collect();
                    db.routes.push(Route {
                        view: Some(name.clone()),
                        key: Some(format!("page {n}")),
                        rows: Some(members.len()),
                        page: Some(n),
                        members,
                        ..Route::new(url, RouteKind::View)
                    });
                }
            }
            // Single route over a (possibly limited) slice: the feed.
            None => {
                let tmpl = v
                    .route
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("view {name} needs a route"))?;
                let members: Vec<usize> = visible
                    .iter()
                    .copied()
                    .take(v.limit.unwrap_or(visible.len()))
                    .collect();
                db.routes.push(Route {
                    view: Some(name.clone()),
                    rows: Some(members.len()),
                    members,
                    ..Route::new(tmpl.to_string(), RouteKind::View)
                });
            }
            Some(_) => unreachable!("grouped views are handled above"),
        }
    }
    Ok(())
}

/// Materialize a view over the objects table (§5 audit gaps 1–3): `match`
/// scopes by path glob (reusing rule globs, not growing the filter
/// language), `filter` type-checks against the object schema, `order_by`
/// is *required* — objects have no natural order, and lexical-by-luck is
/// not a contract — and the route's `members` index into `objects.rows`.
fn build_object_view(
    _cfg: &Config,
    db: &mut SiteDb,
    name: &str,
    v: &View,
    q: &Query,
) -> Result<()> {
    if v.group_by.is_some() || v.paginate.is_some() {
        bail!("view {name}: group_by/paginate on object views is not supported yet");
    }
    let Some(route) = v.route.as_deref() else {
        bail!("view {name} needs a route");
    };
    let order = v
        .order_by
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("view {name}: object views need an order_by (have: name)"))?;
    if order != "name" {
        bail!("view {name}: unknown order_by {order:?} (have: name)");
    }
    let scope = match &v.scope {
        Some(g) => Some(
            globset::Glob::new(g)
                .with_context(|| format!("view {name}: match {g:?}"))?
                .compile_matcher(),
        ),
        None => None,
    };
    let pred = match q.predicate() {
        Some(src) => filter::Filter::parse(&src, &object_schema())
            .with_context(|| format!("view {name}: filter {src:?}"))?,
        None => filter::Filter::always(),
    };
    let mut members: Vec<usize> = db
        .objects
        .rows
        .iter()
        .enumerate()
        .filter(|(_, o)| scope.as_ref().is_none_or(|m| m.is_match(&o.rel)))
        .filter(|(_, o)| pred.eval(*o))
        .map(|(i, _)| i)
        .collect();
    members.sort_by(|&a, &b| {
        let (x, y) = (&db.objects.rows[a], &db.objects.rows[b]);
        x.name.cmp(&y.name).then_with(|| x.rel.cmp(&y.rel))
    });
    db.routes.push(Route {
        view: Some(name.to_string()),
        rows: Some(members.len()),
        members,
        ..Route::new(route.to_string(), RouteKind::View)
    });
    Ok(())
}

/// Order two field values: same-type natural order, Null last. Mixed types
/// cannot occur under a validated `order_by` (the key has one declared type).
fn value_cmp(a: &filter::Value, b: &filter::Value) -> std::cmp::Ordering {
    use filter::Value as V;
    use std::cmp::Ordering::*;
    match (a, b) {
        (V::Str(x), V::Str(y)) => x.cmp(y),
        (V::Int(x), V::Int(y)) => x.cmp(y),
        (V::Bool(x), V::Bool(y)) => x.cmp(y),
        (V::Null, V::Null) => Equal,
        (V::Null, _) => Greater,
        (_, V::Null) => Less,
        _ => Equal,
    }
}

/// Materialize (or resolve, for the routeless/embeddable shape) a view over
/// the tree table: `match` scopes by glob, filters type-check against the
/// page schema, `order_by` is required (`field` or `-field` for descending —
/// a base page field or one declared by any `.schema.toml`, §5b), and only
/// *rendered* pages are rows — static passthrough is not content.
fn build_tree_view(_cfg: &Config, db: &mut SiteDb, name: &str, v: &View, q: &Query) -> Result<()> {
    if v.group_by.is_some() || v.paginate.is_some() {
        bail!("view {name}: group_by/paginate on tree views is not supported yet");
    }
    let order = v
        .order_by
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("view {name}: tree views need an order_by"))?;
    let (key, desc) = match order.strip_prefix('-') {
        Some(k) => (k, true),
        None => (order, false),
    };
    if !crate::db::page_schema().contains_key(key) && !db.schemas.declared().contains_key(key) {
        let mut known: Vec<&str> = crate::db::page_schema().keys().copied().collect();
        known.extend(db.schemas.declared().keys().copied());
        known.sort_unstable();
        bail!(
            "view {name}: order_by names unknown field {key:?}\n  known fields: {}",
            known.join(", ")
        );
    }
    let scope = match &v.scope {
        Some(g) => Some(
            globset::Glob::new(g)
                .with_context(|| format!("view {name}: match {g:?}"))?
                .compile_matcher(),
        ),
        None => None,
    };
    let pred = match q.predicate() {
        Some(src) => filter::Filter::parse(&src, &crate::db::page_schema())
            .with_context(|| format!("view {name}: filter {src:?}"))?,
        None => filter::Filter::always(),
    };
    let mut members: Vec<usize> = db
        .pages
        .rows
        .iter()
        .enumerate()
        .filter(|(_, p)| p.rendered)
        .filter(|(_, p)| scope.as_ref().is_none_or(|m| m.is_match(&p.rel)))
        .filter(|(_, p)| pred.eval(*p))
        .map(|(i, _)| i)
        .collect();
    members.sort_by(|&a, &b| {
        use crate::filter::Row as _;
        let (x, y) = (&db.pages.rows[a], &db.pages.rows[b]);
        let ord = value_cmp(&x.field(key), &y.field(key));
        let ord = if desc { ord.reverse() } else { ord };
        ord.then_with(|| x.rel.cmp(&y.rel))
    });
    members.truncate(v.limit.unwrap_or(usize::MAX));

    if !v.is_materialized() {
        db.views.insert(
            name.to_string(),
            ViewRows {
                layout: v.layout.clone(),
                variant: v.variant.clone(),
                rows: members.len(),
                table: Kind::Tree,
                members,
            },
        );
        return Ok(());
    }
    let Some(route) = v.route.as_deref() else {
        bail!("view {name} needs a route");
    };
    db.routes.push(Route {
        view: Some(name.to_string()),
        rows: Some(members.len()),
        members,
        ..Route::new(route.to_string(), RouteKind::View)
    });
    Ok(())
}

/// Views over the whole route set (the sitemap). Runs after every other route
/// exists, and its `rows` is the count that actually passes its filter.
pub(crate) fn build_star_views(cfg: &Config, db: &mut SiteDb) -> Result<()> {
    for (name, v) in &cfg.views {
        if v.over != "*" {
            continue;
        }
        let tmpl = v
            .route
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("view {name} needs a route"))?;
        let pred = match &v.filter {
            Some(src) => filter::Filter::parse(src, &route_schema())
                .with_context(|| format!("view {name}: filter {src:?}"))?,
            None => filter::Filter::always(),
        };
        let rows = db.routes.iter().filter(|r| pred.eval(*r)).count();
        db.routes.push(Route {
            view: Some(name.clone()),
            rows: Some(rows),
            ..Route::new(tmpl.to_string(), RouteKind::View)
        });
    }
    Ok(())
}

#[cfg(test)]
mod object_view_tests {
    use super::*;
    use crate::db::Object;
    use std::path::PathBuf;

    fn obj(rel: &str) -> Object {
        Object {
            path: PathBuf::from(rel),
            rel: PathBuf::from(rel),
            version: 0,
            url: format!("/{rel}"),
            ext: rel.rsplit('.').next().unwrap_or("").into(),
            name: rel.rsplit('/').next().unwrap_or(rel).into(),
            size: 1,
        }
    }

    fn cfg(views: &str) -> Config {
        let src = format!(
            "root = \".\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
             [collections.objects]\nkind = \"objects\"\n{views}"
        );
        toml::from_str(&src).expect("test config parses")
    }

    #[test]
    fn object_view_scopes_sorts_and_routes() {
        let c = cfg(
            "[views.g]\nover = \"objects\"\nmatch = \"photos/**\"\n\
             order_by = \"name\"\nroute = \"/photos/\"\nlayout = \"gallery\"\n",
        );
        let mut db = SiteDb::default();
        db.objects.rows =
            vec![obj("assets/x.png"), obj("photos/b.png"), obj("photos/a.png")];
        build_views(&c, &mut db).unwrap();
        let r = db.routes.iter().find(|r| r.url == "/photos/").expect("route");
        assert_eq!(r.rows, Some(2));
        // Sorted by name (a before b); the out-of-scope asset is absent.
        assert_eq!(r.members, vec![2, 1]);
    }

    #[test]
    fn object_view_requires_order_by() {
        let c = cfg("[views.g]\nover = \"objects\"\nroute = \"/p/\"\nlayout = \"gallery\"\n");
        let e = build_views(&c, &mut SiteDb::default()).unwrap_err().to_string();
        assert!(e.contains("order_by"), "{e}");
    }

    #[test]
    fn object_filters_typecheck_against_the_object_schema() {
        let c = cfg(
            "[views.g]\nover = \"objects\"\nfilter = \"draft\"\n\
             order_by = \"name\"\nroute = \"/p/\"\nlayout = \"gallery\"\n",
        );
        let e = format!("{:#}", build_views(&c, &mut SiteDb::default()).unwrap_err());
        assert!(e.contains("unknown field `draft`"), "{e}");
    }
}

#[cfg(test)]
mod grouping_tests {
    use super::*;
    use chrono::NaiveDate;

    fn post(date: Option<&str>, tags: &[&str]) -> Post {
        Post {
            date: date.map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap()),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            ..Post::default()
        }
    }

    #[test]
    fn subdivision_chain_accumulates_params() {
        let p = post(Some("2022-03-16"), &[]);
        let chain = vec!["date.year".to_string(), "date.month".to_string()];
        let combos = key_combos(&p, &chain).unwrap();
        assert_eq!(combos.len(), 1);
        let params: Vec<(String, String)> =
            combos[0].iter().flat_map(|k| k.params.clone()).collect();
        assert!(params.contains(&("year".into(), "2022".into())), "{params:?}");
        assert!(params.contains(&("month".into(), "3".into())), "{params:?}");
        assert!(params.contains(&("month_name".into(), "March".into())), "{params:?}");
        // Composite display joins with zero-padded numerics: "2022-03".
        let key: Vec<String> = combos[0].iter().map(|k| k.sort.display()).collect();
        assert_eq!(key.join("-"), "2022-03");
    }

    #[test]
    fn undated_rows_are_absent_from_date_partitions() {
        let p = post(None, &["rust"]);
        assert!(key_combos(&p, &["date.year".into()]).unwrap().is_empty());
        // ...but present in the tag partition.
        assert_eq!(key_combos(&p, &["tags".into()]).unwrap().len(), 1);
    }

    #[test]
    fn tags_multi_key_a_row() {
        let p = post(Some("2022-03-16"), &["c", "rust"]);
        let combos = key_combos(&p, &["tags".into()]).unwrap();
        assert_eq!(combos.len(), 2);
    }

    #[test]
    fn months_sort_numerically_not_lexically() {
        assert!(SortKey::Int(3) < SortKey::Int(12));
        assert_eq!(SortKey::Int(3).display(), "03");
        assert_eq!(SortKey::Int(2022).display(), "2022");
    }
}

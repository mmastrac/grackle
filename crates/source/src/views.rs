//! Views become routes: resolving each declared query into row sets, group
//! partitions (subdivision, §5c) and materialized `Route`s. Split from the
//! table-building half of the database (`db.rs`); `SiteDb::load` calls in
//! here once the tables and row routes exist.

use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;

use crate::config::{Config, Kind, Query, View};
use crate::schema::Schemas;
use grackle_db::filter;
use grackle_db::template;
use grackle_model::{object_schema, route_schema, row_schema, Route, RouteKind, SiteDb, ViewRows};

/// One group key a row contributes under a single `group_by` spec: the typed
/// sort component (years/months order numerically, tags lexically), the
/// display component (joined into `Route.key`), and the parameters the key
/// exposes to route/`title`/`crumb` templates.
#[derive(Clone, Debug)]
pub struct GroupKey {
    sort: SortKey,
    pub params: Vec<(String, String)>,
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

/// The canonical spelling of a `group_by` spec. The date specs were always
/// aliases for schema fields the filter language already had — grouping by
/// tags, by year, by course is ONE operation: group by a typed field.
const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// The group keys a row holds under one spec, read through the same typed
/// field access filters use: a `List` field multi-keys (one group per
/// item), scalars single-key, `Null` means the row is absent from this
/// partition (an undated row under a year grouping; a course-less recipe
/// under a course grouping). Every key exposes `{key}` plus a param named
/// after the field; `month` keeps its display derivative (`{month_name}`)
/// until §5f formatters give it a proper home.
fn group_keys(row: &dyn filter::Row, spec: &str) -> Vec<GroupKey> {
    let field = grackle_model::spec_field(spec);
    let mk = |sort: SortKey, display: String| {
        let mut params = vec![("key".to_string(), display.clone())];
        if field != "key" {
            params.push((field.to_string(), display));
        }
        if field == "month" {
            if let SortKey::Int(m) = sort {
                if (1..=12).contains(&m) {
                    params.push(("month_name".into(), MONTH_NAMES[(m - 1) as usize].into()));
                }
            }
        }
        GroupKey { sort, params }
    };
    match row.field(field) {
        filter::Value::List(items) => items
            .into_iter()
            .map(|t| mk(SortKey::Str(t.clone()), t))
            .collect(),
        filter::Value::Str(s) => vec![mk(SortKey::Str(s.clone()), s)],
        filter::Value::Int(i) => vec![mk(SortKey::Int(i), i.to_string())],
        filter::Value::Bool(b) => vec![mk(SortKey::Str(b.to_string()), b.to_string())],
        filter::Value::Null => Vec::new(),
    }
}

/// Load-time check for a view's group chain: every spec must name a field
/// of the base schema plus any `.schema.toml` declaration (§5b) — the
/// `order_by` discipline applied to grouping, so a typo cannot produce an
/// empty partition silently.
///
/// Two of the three arms were identical the moment there was one row
/// schema; objects remain their own thing, having no front matter to
/// declare fields in.
fn check_group_chain(schemas: &Schemas, name: &str, chain: &[String], kind: Kind) -> Result<()> {
    for spec in chain {
        let field = grackle_model::spec_field(spec);
        let mut known: Vec<&str> = match kind {
            Kind::Objects => object_schema().keys().copied().collect(),
            Kind::Posts | Kind::Tree => {
                let mut v: Vec<&str> = row_schema().keys().copied().collect();
                v.extend(schemas.declared().keys().copied());
                v
            }
        };
        if !known.contains(&field) {
            known.sort_unstable();
            known.dedup();
            bail!(
                "view {name}: group_by names unknown field {field:?}\n  known fields: {}",
                known.join(", ")
            );
        }
    }
    Ok(())
}

/// The composite keys a row belongs to under a subdivision chain — the
/// cartesian product across levels (a list field can multi-key a row;
/// scalar fields contribute at most one each). Empty when the row is
/// absent at any level.
pub fn key_combos(row: &dyn filter::Row, chain: &[String]) -> Vec<Vec<GroupKey>> {
    let mut combos: Vec<Vec<GroupKey>> = vec![Vec::new()];
    for spec in chain {
        let keys = group_keys(row, spec);
        if keys.is_empty() {
            return Vec::new();
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
    combos
}

/// Materialize one route per composite group key. Shared by every base
/// table — grouping never cared what a post or a page was.
///
/// `route_value` maps a `{param}` to the value the URL wears — the seam
/// where a tag's route slug (§6f, `[tags.x] slug`) diverges from its id.
/// Group keys, params and titles keep the id; only the URL is slugged.
fn grouped_routes(
    name: &str,
    tmpl: &str,
    chain: &[String],
    rows: &[(grackle_db::Key, &dyn filter::Row)],
    route_value: &dyn Fn(&str, &str) -> String,
) -> Result<Vec<Route>> {
    let mut groups: BTreeMap<Vec<SortKey>, (Vec<(String, String)>, Vec<grackle_db::Key>)> =
        BTreeMap::new();
    for (i, row) in rows {
        for combo in key_combos(*row, chain) {
            let sort: Vec<SortKey> = combo.iter().map(|k| k.sort.clone()).collect();
            groups
                .entry(sort)
                .or_insert_with(|| {
                    (
                        combo.iter().flat_map(|k| k.params.clone()).collect(),
                        Vec::new(),
                    )
                })
                .1
                .push(i.clone());
        }
    }
    let mut out = Vec::new();
    for (sort, (params, members)) in groups {
        let url = template::render(tmpl, |k| {
            template::param(&params, k).map(|v| route_value(k, &v))
        })?;
        let key = sort
            .iter()
            .map(SortKey::display)
            .collect::<Vec<_>>()
            .join("-");
        out.push(Route {
            view: Some(name.to_string()),
            key: Some(key),
            rows: Some(members.len()),
            params,
            members,
            ..Route::new(url, RouteKind::View)
        });
    }
    Ok(out)
}

/// Which locales a materializing view partitions into (§6f). Default-on:
/// every locale, unless the view opts out with `locales = "default"`.
fn locales_for<'a>(cfg: &'a Config, v: &View) -> Vec<&'a str> {
    match v.locales.as_deref() {
        Some("default") => vec![cfg.i18n.default.as_str()],
        _ => std::iter::once(cfg.i18n.default.as_str())
            .chain(cfg.i18n.locales.iter().map(String::as_str))
            .collect(),
    }
}

/// A route template in one locale. The default locale sits ABOVE the
/// selector, so it wears no prefix.
fn prefixed(cfg: &Config, locale: &str, tmpl: &str) -> String {
    if locale == cfg.i18n.default {
        tmpl.to_string()
    } else {
        format!("/{locale}{tmpl}")
    }
}

/// What a route in one locale records as its own. `None` for the default,
/// which is what `Route.locale` means (§6f) and what filters see as Null.
fn stamp(cfg: &Config, locale: &str) -> Option<String> {
    (locale != cfg.i18n.default).then(|| locale.to_string())
}

/// A view with no route: one row set, and nowhere to hang it but the view.
fn insert_routeless(
    db: &mut SiteDb,
    name: &str,
    v: &View,
    members: Vec<grackle_db::Key>,
    table: Kind,
) {
    db.views.insert(
        name.to_string(),
        ViewRows {
            layout: v.layout.clone(),
            variant: v.variant.clone(),
            rows: members.len(),
            table,
            members,
        },
    );
}

/// The default ordering for dated rows: newest first, undated last, slug as
/// the tiebreak. Was `PostsTable::order`, an index built once at load; it is
/// a comparator now so that losing the table costs nothing (q51).
pub fn chronological(rows: &[grackle_model::Row], a: usize, b: usize) -> std::cmp::Ordering {
    let (x, y) = (&rows[a], &rows[b]);
    match (x.date, y.date) {
        (Some(p), Some(q)) => q.cmp(&p),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
    .then_with(|| x.slug.cmp(&y.slug))
}

/// What a sequence orders by when nothing says otherwise: newest first,
/// undated last, path breaking ties. Adjacency's default, and NOT a view's —
/// a view renders a list, a sequence encodes before-and-after.
fn newest_first() -> Vec<grackle_db::Order> {
    vec![
        grackle_db::Order::desc("date"),
        grackle_db::Order::asc("path"),
    ]
}

/// The order a view asked for, plus the tiebreak every view gets.
///
/// `path` goes last, always: two rows equal on the sort column would
/// otherwise order by whatever the directory walk yielded, which is not an
/// ordering. `path` ALONE is the default, because a tree is a list of files
/// and their paths are the one ordering every row has. A collection whose
/// rows carry dates says so — `order_by = "-date"` — rather than the engine
/// assuming every corpus is a blog.
fn declared_order(
    schemas: &Schemas,
    who: &str,
    spec: Option<&str>,
) -> Result<Vec<grackle_db::Order>> {
    let mut out = Vec::new();
    if let Some(spec) = spec {
        let (key, desc) = match spec.strip_prefix('-') {
            Some(k) => (k, true),
            None => (spec, false),
        };
        if !row_schema().contains_key(key) && !schemas.declared().contains_key(key) {
            let mut known: Vec<&str> = row_schema().keys().copied().collect();
            known.extend(schemas.declared().keys().copied());
            known.sort_unstable();
            known.dedup();
            bail!(
                "{who}: order_by names unknown field {key:?}\n  known fields: {}",
                known.join(", ")
            );
        }
        out.push(grackle_db::Order {
            column: key.to_string(),
            desc,
        });
    }
    out.push(grackle_db::Order::asc("path"));
    Ok(out)
}

/// The sequence `next`/`previous` step through, one per posts collection
/// (q51's ordering decision). "Previous post" means previous *in a
/// sequence*, and a sequence is a set — so the reach is declared, not
/// inherited from whatever index the table happened to carry.
///
/// A declared `adjacency` set brings its filter AND its `order_by`, so
/// `adjacency = "published"` drops drafts by construction. Unset reproduces
/// the old accident exactly: every row of the collection, default locale,
/// newest first — drafts fall off the ends only because they are undated.
pub(crate) fn build_adjacency(cfg: &Config, db: &mut SiteDb, schemas: &Schemas) -> Result<()> {
    let mut out: BTreeMap<String, Vec<grackle_db::Key>> = BTreeMap::new();
    for (cname, c) in &cfg.collections {
        if c.kind != Kind::Posts {
            continue;
        }
        let who = format!("collection {cname}: adjacency");
        let (pred, sort) = match c.adjacency.as_deref() {
            Some(set) => {
                let q = cfg
                    .query(set)
                    .with_context(|| format!("{who} names set {set:?}"))?;
                // A set is rooted at one collection; adjacency inside a
                // collection cannot be defined by a set over a different
                // one. Caught here rather than producing an empty chain.
                if q.base != *cname {
                    bail!(
                        "{who}: set {set:?} is over {:?}, not this collection",
                        q.base
                    );
                }
                let pred = match q.predicate() {
                    Some(src) => filter::Filter::parse(&src, &row_schema())
                        .with_context(|| format!("{who}: filter {src:?}"))?,
                    None => filter::Filter::always(),
                };
                // A declared set brings its filter; its ORDER is only
                // adopted if it states one. Falling through to the view
                // default (`path` ascending) would reverse the sequence,
                // and `neighbors_in` reads position, not dates — "later
                // post" would have meant the one before.
                let order = match q.order_by.as_deref() {
                    Some(spec) => declared_order(schemas, &who, Some(spec))?,
                    None => newest_first(),
                };
                (pred, order)
            }
            // Newest first, which is what `neighbors_in` reads as (newer,
            // older). Unlike a view's, this default is not `path` ascending:
            // a sequence's ORDER is its meaning here, and "previous post"
            // means previous in time until a declared set says otherwise.
            None => (filter::Filter::always(), newest_first()),
        };
        let ix: Vec<grackle_db::Key> = db
            .rows
            .iter()
            .filter(|p| p.collection == *cname)
            // §6f: single-locale, as `PostsTable::order` was — a row's
            // neighbours are in its own language.
            .filter(|p| p.locale == cfg.i18n.default)
            .map(|p| p.key.clone())
            .collect();
        let seq = grackle_db::View::all().filter(pred).order(sort);
        out.insert(cname.clone(), db.rows.view_within(&ix, &seq));
    }
    db.adjacency = out;
    Ok(())
}

pub(crate) fn build_views(cfg: &Config, db: &mut SiteDb, schemas: &Schemas) -> Result<()> {
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
        let Some(base) = cfg.collections.get(&q.base) else {
            continue;
        };
        match base.kind {
            Kind::Objects => build_object_view(cfg, db, name, v, &q)?,
            kind => build_row_view(cfg, db, schemas, name, v, &q, kind)?,
        }
    }
    Ok(())
}

/// Materialize a view over ROWS — posts or tree, one flow.
///
/// These were two functions, and the base collection's KIND decided which one
/// ran, so two views declaring the same thing could behave differently. Every
/// difference that was real has been settled elsewhere: ordering is one rule
/// (`path` unless the view says otherwise), `match` scopes are part of the
/// filter, and `limit` lands in one place. What survives is which index list
/// the view starts from — a set, not a shape.
fn build_row_view(
    cfg: &Config,
    db: &mut SiteDb,
    schemas: &Schemas,
    name: &str,
    v: &View,
    q: &Query,
    kind: Kind,
) -> Result<()> {
    // Parsed and type-checked once per view, not per row: a bad filter is a
    // startup error naming the view.
    let view = grackle_db::View::all()
        .filter(scoped_filter(name, q, &row_schema())?)
        .order(declared_order(
            schemas,
            &format!("view {name}"),
            q.order_by.as_deref(),
        )?);
    // `from = "posts"` has always meant the posts TABLE rather than the one
    // collection that names it — `_posts` and `_drafts` both feed it (q51).
    let base_ix = match kind {
        Kind::Posts => &db.post_ix,
        _ => &db.page_ix,
    };
    let rows = &db.rows;

    // One row set per locale (§6f), built the same way for every locale —
    // including the default, which used to be the special case that read the
    // posts table's index.
    //
    // `rendered` and `!claimed` are no-ops on the posts side: every post is
    // parsed, and only a tree row can be a view's claimed content (q45). They
    // are here because they say what the eligible set IS, rather than which
    // table it came from — which is what let the two flows become one.
    let rows_for = |locale: &str| -> Vec<grackle_db::Key> {
        let eligible: Vec<grackle_db::Key> = base_ix
            .iter()
            .filter_map(|k| rows.get(k))
            .filter(|p| p.rendered && !p.claimed && p.locale == locale)
            .map(|p| p.key.clone())
            .collect();
        rows.view_within(&eligible, &view)
    };

    // No route: one row set, and nowhere to hang it but the view itself.
    if !v.is_materialized() {
        let members: Vec<grackle_db::Key> = rows_for(&cfg.i18n.default)
            .into_iter()
            .take(v.limit.unwrap_or(usize::MAX))
            .collect();
        insert_routeless(db, name, v, members, kind);
        return Ok(());
    }

    // §6f locale-parallel views, DEFAULT-ON (Matt): a materializing row-query
    // view partitions by locale unless it opts out with `locales = "default"`.
    // A locale with no rows materializes nothing: the partition is real, not
    // mirrored.
    let locales = locales_for(cfg, v);

    // Grouped views, possibly a subdivision chain (§5c): a grouped view
    // `over` a grouped view refines the parent's partition, and the group keys
    // accumulate — GROUP BY year, month, expressed compositionally.
    if v.group_by.is_some() {
        let tmpl = v
            .route
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("view {name} needs a route"))?;
        let chain = cfg.group_specs(name);
        check_group_chain(schemas, name, &chain, kind)?;
        // §6f enum records: URLs wear the record's slug for ANY grouped field
        // (tags, courses, …); keys and titles keep the id.
        let leaf = chain
            .last()
            .map(|s| grackle_model::spec_field(s).to_string());
        let route_value = |k: &str, v: &str| -> String {
            let field = if k == "key" { leaf.as_deref() } else { Some(k) };
            match field {
                Some(f) => cfg.record_slug(f, v).to_string(),
                None => v.to_string(),
            }
        };
        for locale in &locales {
            let row_ix = rows_for(locale);
            let grouped: Vec<(grackle_db::Key, &dyn filter::Row)> = row_ix
                .iter()
                .filter_map(|k| rows.get(k).map(|r| (k.clone(), r as &dyn filter::Row)))
                .collect();
            let mut routes = grouped_routes(
                name,
                &prefixed(cfg, locale, tmpl),
                &chain,
                &grouped,
                &route_value,
            )?;
            for r in &mut routes {
                r.locale = stamp(cfg, locale);
            }
            db.routes.extend(routes);
        }
        return Ok(());
    }

    // Paginated list. Was posts-only, on no stronger grounds than the tree
    // flow never having been written to do it.
    if let Some(per) = v.paginate.map(|p| p.max(1)) {
        for locale in &locales {
            let row_ix = rows_for(locale);
            if row_ix.is_empty() && *locale != cfg.i18n.default {
                continue;
            }
            for n in 1..=row_ix.len().div_ceil(per) {
                let tmpl = if n == 1 {
                    v.routes.first()
                } else {
                    v.routes.get(1).or_else(|| v.routes.first())
                };
                let Some(tmpl) = tmpl else { continue };
                let url = template::render(&prefixed(cfg, locale, tmpl), |k| match k {
                    "n" => Some(n.to_string()),
                    _ => None,
                })?;
                let members: Vec<grackle_db::Key> = row_ix
                    .iter()
                    .skip(per * (n - 1))
                    .take(per)
                    .cloned()
                    .collect();
                db.routes.push(Route {
                    view: Some(name.to_string()),
                    key: Some(format!("page {n}")),
                    rows: Some(members.len()),
                    page: Some(n),
                    members,
                    locale: stamp(cfg, locale),
                    ..Route::new(url, RouteKind::View)
                });
            }
        }
        return Ok(());
    }

    // Single route over a (possibly limited) slice: the feed — which is how
    // /fr/atom.xml falls out of the default (§6f).
    let Some(route) = v.route.as_deref() else {
        bail!("view {name} needs a route");
    };
    for locale in &locales {
        let row_ix = rows_for(locale);
        // No rows in this locale = no page (the partition is real).
        if row_ix.is_empty() && *locale != cfg.i18n.default {
            continue;
        }
        let members: Vec<grackle_db::Key> = row_ix
            .iter()
            .take(v.limit.unwrap_or(row_ix.len()))
            .cloned()
            .collect();
        db.routes.push(Route {
            view: Some(name.to_string()),
            rows: Some(members.len()),
            members,
            locale: stamp(cfg, locale),
            ..Route::new(prefixed(cfg, locale, route), RouteKind::View)
        });
    }
    Ok(())
}

/// Materialize a view over the objects table (§5 audit gaps 1–3): `match`
/// scopes by path glob (reusing rule globs, not growing the filter
/// language), `filter` type-checks against the object schema, `order_by`
/// is *required* — objects have no natural order, and lexical-by-luck is
/// not a contract — and the route's `members` index into `objects.rows`.
/// A view's declared filter, narrowed by its `match` chain.
///
/// The chain's globs are CONJOINED: a row must satisfy every one, so a child
/// narrows within its parent's subtree and can never widen out of it (§5c).
/// They compile to `glob(path, ...)` rather than running as a separate pass,
/// which is what makes a scope and a filter one thing — composable, checked
/// by one type-checker, and applied wherever the filter is applied.
fn scoped_filter(name: &str, q: &Query, schema: &filter::Schema) -> Result<filter::Filter> {
    let mut f = match q.predicate() {
        Some(src) => filter::Filter::parse(&src, schema)
            .with_context(|| format!("view {name}: filter {src:?}"))?,
        None => filter::Filter::always(),
    };
    for g in &q.scopes {
        let src = format!("glob(path, {g:?})");
        f = f.and(
            filter::Filter::parse(&src, schema)
                .with_context(|| format!("view {name}: match {g:?}"))?,
        );
    }
    Ok(f)
}

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
    // §6f: objects carry no locale — an object view never multiplies, and
    // saying otherwise is a config error, not a silent ignore.
    if v.locales.is_some() {
        bail!("view {name}: objects carry no locale; object views cannot declare locales");
    }
    let Some(route) = v.route.as_deref() else {
        bail!("view {name} needs a route");
    };
    let order = q.order_by.as_deref().ok_or_else(|| {
        anyhow::anyhow!("view {name}: object views need an order_by (have: name)")
    })?;
    if order != "name" {
        bail!("view {name}: unknown order_by {order:?} (have: name)");
    }
    let view = grackle_db::View::all()
        .filter(scoped_filter(name, q, &object_schema())?)
        // `path` breaks the tie: two objects can share a filename, and
        // directory order is not an ordering.
        .order(vec![
            grackle_db::Order::asc("name"),
            grackle_db::Order::asc("path"),
        ]);
    let members: Vec<grackle_db::Key> = db.objects.rows.view(&view);
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
        db.routes.push(Route {
            view: Some(name.clone()),
            ..Route::new(tmpl.to_string(), RouteKind::View)
        });
    }
    Ok(())
}

/// Resolve each star view's members, once the route list is final.
///
/// A star view ranges over ROUTES, so its members are positions into
/// `db.routes` rather than into the row store — the one place in the engine
/// where that is true, and true because `over = "*"` says so.
///
/// Deferred to here because those positions are only stable once the list
/// stops growing and has been sorted. Resolving during `build_star_views`
/// measured a partial list: views build in name order, so `sitemap` saw
/// `search`'s route and would not have seen it the other way round. Both
/// filters happen to exclude the other's route by extension, which is why
/// nothing was visibly wrong.
pub(crate) fn resolve_star_views(cfg: &Config, db: &mut SiteDb) -> Result<()> {
    for (name, v) in &cfg.views {
        if v.over != "*" {
            continue;
        }
        let pred = match &v.filter {
            Some(src) => filter::Filter::parse(src, &route_schema())
                .with_context(|| format!("view {name}: filter {src:?}"))?,
            None => filter::Filter::always(),
        };
        let members = db.routes.select(&pred);
        let Some(at) = db
            .routes
            .iter()
            .find(|r| r.kind == RouteKind::View && r.view.as_deref() == Some(name.as_str()))
            .map(|r| r.id.clone())
        else {
            continue;
        };
        if let Some(r) = db.routes.get_mut(&at) {
            r.rows = Some(members.len());
            r.route_members = members;
        }
    }
    Ok(())
}

#[cfg(test)]
mod object_view_tests {
    use super::*;
    use grackle_model::Row;
    use std::path::PathBuf;

    /// An object row: a path, a URL, and nothing rendered. `name`, `ext` and
    /// `stem` all derive from `rel`, so there is nothing else to set.
    fn obj(rel: &str) -> Row {
        Row {
            key: grackle_db::Key::new(rel),
            path: PathBuf::from(rel),
            rel: PathBuf::from(rel),
            url: format!("/{rel}"),
            size: 1,
            rendered: false,
            ..Default::default()
        }
    }

    fn cfg(views: &str) -> Config {
        let src = format!(
            "root = \".\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
             [[collections]]\nname = \"objects\"\nkind = \"objects\"\n{views}"
        );
        Config::from_toml(&src).expect("test config parses")
    }

    #[test]
    fn object_view_scopes_sorts_and_routes() {
        let c = cfg("[routes.g]\nfrom = \"objects\"\nmatch = \"photos/**\"\n\
             order_by = \"name\"\npath = \"/photos/\"\nlayout = \"gallery\"\n");
        let mut db = SiteDb::default();
        db.objects.rows = grackle_db::Table::new(vec![
            obj("assets/x.png"),
            obj("photos/b.png"),
            obj("photos/a.png"),
        ]);
        build_views(&c, &mut db, &Schemas::new(row_schema())).unwrap();
        let r = db
            .routes
            .iter()
            .find(|r| r.url == "/photos/")
            .expect("route");
        assert_eq!(r.rows, Some(2));
        // Sorted by name (a before b); the out-of-scope asset is absent.
        assert_eq!(
            r.members
                .iter()
                .map(|k| k.as_str().to_string())
                .collect::<Vec<_>>(),
            ["photos/a.png", "photos/b.png"]
        );
    }

    #[test]
    fn object_view_requires_order_by() {
        let c = cfg("[routes.g]\nfrom = \"objects\"\npath = \"/p/\"\nlayout = \"gallery\"\n");
        let e = build_views(&c, &mut SiteDb::default(), &Schemas::new(row_schema()))
            .unwrap_err()
            .to_string();
        assert!(e.contains("order_by"), "{e}");
    }

    #[test]
    fn object_filters_typecheck_against_the_object_schema() {
        let c = cfg("[routes.g]\nfrom = \"objects\"\nwhere = \"draft\"\n\
             order_by = \"name\"\npath = \"/p/\"\nlayout = \"gallery\"\n");
        let e = format!(
            "{:#}",
            build_views(&c, &mut SiteDb::default(), &Schemas::new(row_schema())).unwrap_err()
        );
        assert!(e.contains("unknown field `draft`"), "{e}");
    }
}

/// A view's ordering. `path` ascending unless the view names a column, and
/// `path` as the final tiebreak either way.
#[cfg(test)]
mod posts_order_tests {
    use super::*;
    use chrono::NaiveDate;
    use grackle_model::Row;

    fn post(url: &str, date: &str, order: Option<i64>) -> Row {
        Row {
            collection: "notes".into(),
            url: url.into(),
            date: NaiveDate::parse_from_str(date, "%Y-%m-%d").ok(),
            order,
            // The locale filter is explicit in the view now (it used to
            // ride along inside `posts.order`), so a fixture row has to
            // carry the default locale to be visible at all.
            locale: "en".into(),
            slug: url.trim_matches('/').into(),
            // A row's key is its path, so fixtures need distinct ones or
            // they are all the same row as far as the table is concerned.
            rel: std::path::PathBuf::from(format!("{}.md", url.trim_matches('/'))),
            // Every post is a rendered row; the loader hardcodes it. The
            // fixture has to say so now that eligibility is a predicate
            // rather than a consequence of which flow you landed in.
            rendered: true,
            ..Row::default()
        }
    }

    /// No ordering index is seeded, because none exists: since q51 the
    /// view derives its own ordering from the rows.
    fn db() -> SiteDb {
        SiteDb::seed(
            vec![
                post("/a/", "2026-01-10", Some(1)), // oldest, pinned first
                post("/b/", "2026-03-05", None),
                post("/c/", "2026-06-21", None),
                post("/d/", "2026-07-19", Some(9)), // newest, pinned last
            ],
            true,
        )
    }

    fn cfg(clauses: &str) -> Config {
        let src = format!(
            "root = \".\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
             [[collections]]\nname = \"notes\"\nkind = \"posts\"\nsource = \"_posts\"\n\
             filename_formats = [\"{{year}}-{{month}}-{{day}}-{{slug}}\"]\n\
             [routes.g]\nfrom = \"notes\"\npath = \"/g/\"\nlayout = \"listing\"\n{clauses}"
        );
        Config::from_toml(&src).expect("test config parses")
    }

    fn members(clauses: &str) -> Vec<String> {
        let (c, mut db) = (cfg(clauses), db());
        build_views(&c, &mut db, &Schemas::new(row_schema())).unwrap();
        let r = db.routes.iter().find(|r| r.url == "/g/").expect("route");
        r.members
            .iter()
            .filter_map(|k| db.rows.get(k))
            .map(|r| r.url.clone())
            .collect()
    }

    /// A view with no `order_by` orders by PATH. The engine used to assume
    /// every corpus was a blog and sort newest-first; a tree is a list of
    /// files, and their paths are the one ordering every row has. A posts
    /// collection asks for dates.
    #[test]
    fn no_order_by_means_path_order() {
        assert_eq!(members(""), ["/a/", "/b/", "/c/", "/d/"]);
    }

    #[test]
    fn a_posts_view_asks_for_dates() {
        assert_eq!(
            members("order_by = \"-date\"\n"),
            ["/d/", "/c/", "/b/", "/a/"]
        );
    }

    /// `path` is the last key, always, so rows tied on the declared column
    /// order by their file rather than by whatever the walk yielded. Here
    /// `/b/` and `/c/` declare no `order`, tie at Null, and fall to path.
    #[test]
    fn ties_on_the_declared_column_fall_through_to_path() {
        assert_eq!(
            members("order_by = \"order\"\n"),
            ["/a/", "/d/", "/b/", "/c/"]
        );
    }

    #[test]
    fn order_by_names_a_field_or_it_is_a_load_error() {
        let e = build_views(
            &cfg("order_by = \"ordre\"\n"),
            &mut db(),
            &Schemas::new(row_schema()),
        )
        .unwrap_err()
        .to_string();
        assert!(e.contains("unknown field \"ordre\""), "{e}");
        assert!(e.contains("order"), "the diagnostic lists what exists: {e}");
    }
}

#[cfg(test)]
mod grouping_tests {
    use super::*;
    use chrono::NaiveDate;
    use grackle_model::Row;

    fn post(date: Option<&str>, tags: &[&str]) -> Row {
        Row {
            date: date.map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap()),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            ..Row::default()
        }
    }

    #[test]
    fn subdivision_chain_accumulates_params() {
        let p = post(Some("2022-03-16"), &[]);
        let chain = vec!["date.year".to_string(), "date.month".to_string()];
        let combos = key_combos(&p, &chain);
        assert_eq!(combos.len(), 1);
        let params: Vec<(String, String)> =
            combos[0].iter().flat_map(|k| k.params.clone()).collect();
        assert!(
            params.contains(&("year".into(), "2022".into())),
            "{params:?}"
        );
        assert!(params.contains(&("month".into(), "3".into())), "{params:?}");
        assert!(
            params.contains(&("month_name".into(), "March".into())),
            "{params:?}"
        );
        // Composite display joins with zero-padded numerics: "2022-03".
        let key: Vec<String> = combos[0].iter().map(|k| k.sort.display()).collect();
        assert_eq!(key.join("-"), "2022-03");
    }

    #[test]
    fn undated_rows_are_absent_from_date_partitions() {
        let p = post(None, &["rust"]);
        assert!(key_combos(&p, &["date.year".into()]).is_empty());
        // ...but present in the tag partition.
        assert_eq!(key_combos(&p, &["tags".into()]).len(), 1);
    }

    #[test]
    fn tags_multi_key_a_row() {
        let p = post(Some("2022-03-16"), &["c", "rust"]);
        let combos = key_combos(&p, &["tags".into()]);
        assert_eq!(combos.len(), 2);
    }

    #[test]
    fn months_sort_numerically_not_lexically() {
        assert!(SortKey::Int(3) < SortKey::Int(12));
        assert_eq!(SortKey::Int(3).display(), "03");
        assert_eq!(SortKey::Int(2022).display(), "2022");
    }

    /// The generalization: grouping by a schema field is the same operation
    /// as grouping by tags — Str single-keys, Null is absent.
    #[test]
    fn any_typed_field_groups() {
        use grackle_model::Row;
        use std::path::PathBuf;
        let mut p = Row {
            path: PathBuf::new(),
            rel: PathBuf::from("recipes/carbonara.md"),
            version: 0,
            url: "/recipes/carbonara/".into(),
            rendered: true,
            size: 0,
            title: Some("Carbonara".into()),
            layout: None,
            description: None,
            order: None,
            date: None,
            tags: Vec::new(),
            toc: false,
            theme: None,
            shell: None,
            draft: false,
            hidden: false,
            noindex: false,
            fields: Default::default(),
            images: Default::default(),
            locale: "en".into(),
            logical: "recipes/carbonara.md".into(),
            claimed: false,
            ..Default::default()
        };
        p.fields
            .insert("course".into(), filter::Value::Str("dinner".into()));
        let combos = key_combos(&p, &["course".into()]);
        assert_eq!(combos.len(), 1);
        let params = &combos[0][0].params;
        assert!(
            params.contains(&("key".into(), "dinner".into())),
            "{params:?}"
        );
        assert!(
            params.contains(&("course".into(), "dinner".into())),
            "{params:?}"
        );

        // No course: absent from the partition, same as undated-under-year.
        p.fields.clear();
        assert!(key_combos(&p, &["course".into()]).is_empty());

        // And the point of q51 step 3: `date.year` over a PAGE. Grouping
        // never cared what a post or a page was — until now only one of
        // them could hold the date the spec reads. An undated page is
        // absent from the year partition, exactly as an undated post is.
        assert!(key_combos(&p, &["date.year".into()]).is_empty());
        p.date = chrono::NaiveDate::from_ymd_opt(2026, 7, 1);
        let combos = key_combos(&p, &["date.year".into(), "date.month".into()]);
        assert_eq!(combos.len(), 1);
        let params: Vec<_> = combos[0].iter().flat_map(|k| k.params.clone()).collect();
        assert!(
            params.contains(&("year".into(), "2026".into())),
            "{params:?}"
        );
        assert!(params.contains(&("month".into(), "7".into())), "{params:?}");
    }

    #[test]
    fn date_specs_are_field_aliases() {
        assert_eq!(grackle_model::spec_field("date.year"), "year");
        assert_eq!(grackle_model::spec_field("date.month"), "month");
        assert_eq!(grackle_model::spec_field("course"), "course");
        // The month display derivative survives the generalization.
        let p = post(Some("2022-12-16"), &[]);
        let keys = group_keys(&p, "date.month");
        assert!(keys[0]
            .params
            .contains(&("month_name".into(), "December".into())));
    }
}

/// What `next`/`previous` step through (q51). The two properties here were
/// both real bugs once, and the merge changes which mechanism guarantees
/// them: the collection anchor used to be a filter inside `neighbors`, and
/// is now structural — one sequence per collection.
#[cfg(test)]
mod adjacency_tests {
    use super::*;
    use chrono::NaiveDate;
    use grackle_model::Row;

    fn post(collection: &str, url: &str, date: Option<&str>, draft: bool) -> Row {
        Row {
            rel: std::path::PathBuf::from(format!(
                "{collection}/{}.md",
                url.trim_matches('/').replace('/', "-")
            )),
            collection: collection.into(),
            url: url.into(),
            slug: url.trim_matches('/').replace('/', "-"),
            date: date.and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()),
            draft,
            locale: "en".into(),
            ..Row::default()
        }
    }

    fn db_with(rows: Vec<Row>) -> SiteDb {
        SiteDb::seed(rows, true)
    }

    fn cfg(extra: &str) -> Config {
        Config::from_toml(&format!(
            "root = \".\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
             [[collections]]\nname = \"posts\"\nkind = \"posts\"\nsource = \"_posts\"\n\
             filename_formats = [\"{{year}}-{{month}}-{{day}}-{{slug}}\"]\n{extra}"
        ))
        .expect("test config parses")
    }

    fn seq(db: &SiteDb, collection: &str) -> Vec<String> {
        db.adjacency[collection]
            .iter()
            .filter_map(|k| db.rows.get(k))
            .map(|r| r.url.clone())
            .collect()
    }

    /// Two dated collections interleave in one table, so walking a shared
    /// index made a blog post's neighbour a note — measured on a real
    /// two-collection site, the January blog post linked February's and
    /// April's *notes*. One sequence per collection makes that unable to
    /// recur, rather than filtered out after the fact.
    #[test]
    fn each_collection_gets_its_own_sequence() {
        let c = cfg("[[collections]]\nname = \"notes\"\nkind = \"posts\"\n\
                     source = \"_notes\"\nfilename_formats = [\"{year}-{month}-{day}-{slug}\"]\n");
        let mut db = db_with(vec![
            post("posts", "/blog/jan/", Some("2026-01-01"), false),
            post("notes", "/notes/feb/", Some("2026-02-01"), false),
            post("posts", "/blog/mar/", Some("2026-03-01"), false),
            post("notes", "/notes/apr/", Some("2026-04-01"), false),
        ]);
        build_adjacency(&c, &mut db, &Schemas::new(row_schema())).unwrap();
        assert_eq!(seq(&db, "posts"), ["/blog/mar/", "/blog/jan/"]);
        assert_eq!(seq(&db, "notes"), ["/notes/apr/", "/notes/feb/"]);
    }

    /// The honest version of the draft story. Undeclared, a DATED draft
    /// really is someone's later post — it only fell out before because
    /// drafts are usually undated. Declaring the set fixes it by rule.
    #[test]
    fn a_declared_set_drops_drafts_by_construction() {
        let rows = || {
            vec![
                post("posts", "/blog/jan/", Some("2026-01-01"), false),
                post("posts", "/blog/feb/", Some("2026-02-01"), true), // dated DRAFT
                post("posts", "/blog/mar/", Some("2026-03-01"), false),
            ]
        };

        // Undeclared: the accident, stated plainly.
        let mut db = db_with(rows());
        build_adjacency(&cfg(""), &mut db, &Schemas::new(row_schema())).unwrap();
        assert_eq!(
            seq(&db, "posts"),
            ["/blog/mar/", "/blog/feb/", "/blog/jan/"],
            "a dated draft rides the chain when nothing says otherwise"
        );

        // Declared: `published` carries `!draft`, so the draft is simply
        // not in the sequence and January's later post is March.
        let mut db = db_with(rows());
        let c = cfg(
            "adjacency = \"published\"\n[sets.published]\nfrom = \"posts\"\nwhere = \"!draft\"\n",
        );
        build_adjacency(&c, &mut db, &Schemas::new(row_schema())).unwrap();
        assert_eq!(seq(&db, "posts"), ["/blog/mar/", "/blog/jan/"]);
    }

    /// A set over a DIFFERENT collection would silently produce an empty
    /// chain; it is a load error naming both instead.
    #[test]
    fn adjacency_set_must_be_over_this_collection() {
        let c = Config::from_toml(
            "root = \".\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
             [[collections]]\nname = \"posts\"\nkind = \"posts\"\nsource = \"_posts\"\n\
             filename_formats = [\"{year}-{month}-{day}-{slug}\"]\nadjacency = \"elsewhere\"\n\
             [[collections]]\nname = \"notes\"\nkind = \"posts\"\nsource = \"_notes\"\n\
             filename_formats = [\"{year}-{month}-{day}-{slug}\"]\n\
             [sets.elsewhere]\nfrom = \"notes\"\n",
        )
        .unwrap();
        let e = build_adjacency(&c, &mut db_with(vec![]), &Schemas::new(row_schema()))
            .unwrap_err()
            .to_string();
        assert!(e.contains("not this collection"), "{e}");
    }
}

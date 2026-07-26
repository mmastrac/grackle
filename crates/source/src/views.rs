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
use grackle_model::{object_schema, AxisMember, route_schema, row_schema, Route, RouteKind, SiteDb, ViewRows};

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
        // No row column is a double (it is a computed-expression type only),
        // so grouping by one never happens; grouped by its string form if it
        // somehow did, rather than silently dropped.
        filter::Value::Double(d) => vec![mk(SortKey::Str(d.to_string()), d.to_string())],
        filter::Value::Bool(b) => vec![mk(SortKey::Str(b.to_string()), b.to_string())],
        filter::Value::Null => Vec::new(),
    }
}

/// Load-time check for a view's group chain: every spec must name a field of
/// the base's own vocabulary — the `order_by` discipline applied to grouping,
/// so a typo cannot produce an empty partition silently.
///
/// The vocabulary arrives as a parameter because it belongs to the BASE, not
/// to the kind. `Base::resolve` is the single place that decides what a view's
/// expressions may name (§5c); this function held the second copy of that
/// decision, and its objects arm was unreachable — the dispatch sent objects
/// to a materializer that refused `group_by` before ever calling this.
fn check_group_chain(name: &str, chain: &[String], schema: &filter::Schema) -> Result<()> {
    let mut known: Vec<&str> = schema.keys().copied().collect();
    known.sort_unstable();
    known.dedup();
    for spec in chain {
        let field = grackle_model::spec_field(spec);
        if !known.contains(&field) {
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
    v: &View,
    member: &Option<AxisMember>,
    tmpls: &[String],
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
        let key = sort
            .iter()
            .map(SortKey::display)
            .collect::<Vec<_>>()
            .join("-");
        // `{n}` is the page; every other placeholder is a group param. One
        // renderer for both shapes, so a paginated group's URL cannot be built
        // by a different rule than an unpaginated one's.
        let url = |tmpl: &str, n: Option<usize>| -> Result<String> {
            template::render(tmpl, |k| match k {
                "n" => n.map(|n| n.to_string()),
                _ => template::param(&params, k).map(|v| route_value(k, &v)),
            })
        };
        let route = |url: String, page: Option<usize>, members: Vec<grackle_db::Key>| Route {
            fields: view_fields(v),
            axis: member.clone(),
            view: Some(name.to_string()),
            // The group key, not the page number: `{key}` in a title or crumb
            // names the partition, and `page` carries the pagination.
            key: Some(key.clone()),
            rows: Some(members.len()),
            page,
            params: params.clone(),
            members,
            ..Route::new(url, RouteKind::View)
        };
        match v.paginate.map(|p| p.max(1)) {
            // A grouped view paginates INSIDE each group: the partition says
            // which rows, pagination says how many to a page. `paginate` used
            // to be read only on the ungrouped path, so a grouped view that
            // declared it got one unpaginated route and no warning.
            //
            // This is not q30. That question is pagination × SUBDIVISION — a
            // pageable parent whose children subdivide off its root, where the
            // two share a URL namespace. `config.query` still refuses to
            // compose over a paginated route, so a grouped-and-paginated view
            // remains a leaf.
            Some(per) => {
                for n in 1..=members.len().div_ceil(per).max(1) {
                    let tmpl = if n == 1 { &tmpls[0] } else { &tmpls[1] };
                    let page: Vec<grackle_db::Key> = members
                        .iter()
                        .skip(per * (n - 1))
                        .take(per)
                        .cloned()
                        .collect();
                    out.push(route(url(tmpl, Some(n))?, Some(n), page));
                }
            }
            None => out.push(route(url(&tmpls[0], None)?, None, members)),
        }
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
/// the tiebreak. A comparator, not an index, so losing the table costs
/// nothing (q51).
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
fn declared_order(known: &[&str], who: &str, spec: Option<&str>) -> Result<Vec<grackle_db::Order>> {
    let mut out = Vec::new();
    if let Some(spec) = spec {
        let (key, desc) = match spec.strip_prefix('-') {
            Some(k) => (k, true),
            None => (spec, false),
        };
        if !known.contains(&key) {
            let mut known: Vec<&str> = known.to_vec();
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

/// The chronological sequence the `grackle explain` diagnostic reports as a
/// row's newer/older neighbours, one per posts collection, default locale,
/// newest first.
///
/// This is no longer what a rendered page's "earlier/later post" steps — those
/// are declared relations now (§6g), computed per row by the engine over
/// `published`. The old collection-level `adjacency` set retired with them;
/// this stays only as the CLI's raw table walk.
pub(crate) fn build_adjacency(cfg: &Config, db: &mut SiteDb, _schemas: &Schemas) -> Result<()> {
    let mut out: BTreeMap<String, Vec<grackle_db::Key>> = BTreeMap::new();
    for (cname, c) in &cfg.collections {
        if c.kind != Kind::Posts {
            continue;
        }
        let ix: Vec<grackle_db::Key> = db
            .rows
            .iter()
            .filter(|p| p.collection == *cname)
            // §6f: single-locale — a row's neighbours are in its own language.
            .filter(|p| p.locale == cfg.i18n.default)
            .map(|p| p.key.clone())
            .collect();
        let seq = grackle_db::View::all().order(newest_first());
        out.insert(cname.clone(), db.rows.view_within(&ix, &seq));
    }
    db.adjacency = out;
    Ok(())
}

/// A view's own declarations, as route fields (§4e).
///
/// `[routes.x] noindex = true` is the config saying something about the route,
/// and `[html.head.meta]` reads it by the same name a row would — so the two
/// surfaces answer one expression. The engine still spells `noindex` here,
/// which is the last of it: the honest end state is `fields = { noindex =
/// true }` on the route, once a view can declare arbitrary fields.
fn view_fields(v: &View) -> BTreeMap<String, filter::Value> {
    let mut f = BTreeMap::new();
    if v.noindex {
        f.insert("noindex".to_string(), filter::Value::Bool(true));
    }
    f
}

pub(crate) fn build_views(cfg: &Config, db: &mut SiteDb, schemas: &Schemas) -> Result<()> {
    for (name, v) in &cfg.views {
        // `over = "*"` views read the finished route set, so they run in a
        // second pass (see build_star_views). Views iterate in name order, so
        // inline would measure a partial list.
        if v.over.is_star() {
            continue;
        }
        // Both named queries (`published`) and embedded views (`latest`) still
        // have to resolve, so a typo in `over` is a startup error either way.
        let q = cfg.query(name)?;
        // Dispatch on the base collection's KIND, never its name: a posts
        // collection may be called anything (§7a names one `notes`). A union's
        // members share a kind (checked in `Config::check_base`), so the first
        // answers for all of them.
        let Some(kind) = q
            .base
            .first()
            .and_then(|n| cfg.collections.get(n))
            .map(|c| c.kind)
        else {
            continue;
        };
        let base = Base::resolve(schemas, name, &q, kind)?;
        // q53: the axis is the OUTERMOST dimension — a view on one materializes
        // once per member, and everything below it (locale, grouping,
        // pagination) happens within each. Ordering it outermost is what keeps
        // the branches below a substitution rather than a rewrite.
        for member in axis_members(cfg, name, v)? {
            build_view(cfg, db, name, v, &q, kind, base.clone(), member)?;
        }
    }
    // §4d: an inherited route with nothing to show does not materialize. The
    // base config may not mint a URL the author did not ask for, and a site
    // with no `_posts/` never asked for an empty `/blog/` or a feed with no
    // entries. A route the SITE declared still materializes empty — declaring
    // it IS asking, and an empty listing you wrote is a fact about your
    // content, not a stray page.
    db.routes.retain(|r| {
        let Some(v) = r.view.as_deref().and_then(|n| cfg.views.get(n)) else {
            return true; // a row's own route, not a view's
        };
        !v.inherited || v.over.is_star() || r.rows.is_none_or(|n| n > 0)
    });
    Ok(())
}

/// What a view's base contributes to materialization: the vocabulary its
/// expressions type-check against, the predicate that scopes it to its own
/// rows, and whether those rows are PARSED rows.
///
/// These three are the whole of what used to be a second materializer. Objects
/// differ from posts and tree in exactly them — a narrower vocabulary (no front
/// matter, so `where = "draft"` on a gallery stays the load error §5b wants),
/// and bytes rather than parsed rows. Writing them as parameters is what makes
/// `group_by`, `paginate` and subdivision work over objects: they were never
/// unsupported, only unreachable, and `check_group_chain` had carried a dead
/// objects arm to prove it.
///
/// Membership used to be the third difference, and is not any more. Objects
/// scoped to the one collection named; rows ranged over every collection of
/// their KIND, so `from = "notes"` quietly meant the whole posts table. Now
/// both mean *the collections `from` names* (§5c), and a site that wants the
/// old union writes it: `from = ["posts", "drafts"]`.
#[derive(Clone)]
struct Base {
    schema: filter::Schema,
    membership: filter::Filter,
    /// `rendered`, `claimed` and `locale` are properties of a parsed row. An
    /// object is never rendered, cannot be a view's claimed content (q45) and
    /// carries no locale (§6f) — so row eligibility, applied to objects, would
    /// exclude every one of them.
    parsed: bool,
}

impl Base {
    fn resolve(schemas: &Schemas, name: &str, q: &Query, kind: Kind) -> Result<Base> {
        // One rule for every base: the row's collection is one of the ones
        // `from` named. It parses against the FULL row schema even for objects,
        // because `collection` is a column only that one names — while the
        // author's own filter type-checks against the narrow vocabulary below
        // (§3, q51), which is what keeps `where = "draft"` on a gallery an error.
        let membership = filter::Filter::parse(&members_clause(&q.base), &row_schema())
            .with_context(|| format!("view {name}: base {:?}", q.base))?;
        Ok(match kind {
            Kind::Objects => Base {
                schema: object_schema(),
                membership,
                parsed: false,
            },
            _ => Base {
                // Built-ins plus every declared field, so `where` sees exactly
                // what `order_by`, `group_by` and a relation's `rank` see.
                schema: schemas.row_filter_schema(),
                membership,
                parsed: true,
            },
        })
    }
}

/// `collection == a || collection == b` over the collections a view's `from`
/// named. Empty names nothing, which is not the same as naming everything.
fn members_clause(names: &[String]) -> String {
    if names.is_empty() {
        return "false".to_string();
    }
    names
        .iter()
        .map(|n| format!("collection == {n:?}"))
        .collect::<Vec<_>>()
        .join(" || ")
}

/// The axis members a view materializes across, or a single `None` for a view
/// on no axis — so the caller loops either way and there is no second path for
/// the ordinary case.
fn axis_members(cfg: &Config, name: &str, v: &View) -> Result<Vec<Option<AxisMember>>> {
    let Some(axis_name) = v.axis.as_deref() else {
        return Ok(vec![None]);
    };
    let Some(axis) = cfg.axes.get(axis_name) else {
        let known: Vec<&str> = cfg.axes.keys().map(String::as_str).collect();
        bail!(
            "view {name}: axis = {axis_name:?} names no axis\n  declared axes: {}",
            if known.is_empty() {
                "(none)".into()
            } else {
                known.join(", ")
            }
        );
    };
    // The route has to spend the axis somewhere, or every member renders at one
    // URL and five of the six are lost to a collision. Checked here because
    // this is where the two halves — the axis and the path that allocates it —
    // are both in hand.
    let placeholder = format!("{{{axis_name}}}");
    if !v
        .route
        .iter()
        .chain(v.routes.iter())
        .any(|t| t.contains(&placeholder))
    {
        bail!(
            "view {name}: axis = {axis_name:?} but no path spends it — give the \
             path a {placeholder} segment, or the members would collide on one URL"
        );
    }
    Ok(axis
        .values
        .iter()
        .map(|value| {
            Some(AxisMember {
                axis: axis_name.to_string(),
                value: value.clone(),
                field: axis.field.clone(),
                canonical: axis.canonical() == Some(value.as_str()),
            })
        })
        .collect())
}

/// Materialize a view — one flow for every base.
///
/// A view differs from another only in which index list it starts from: a set,
/// not a shape. Ordering, `match` scopes, `limit`, grouping, pagination and the
/// locale axis are one rule each, applied here for every base.
fn build_view(
    cfg: &Config,
    db: &mut SiteDb,
    name: &str,
    v: &View,
    q: &Query,
    kind: Kind,
    base: Base,
    member: Option<AxisMember>,
) -> Result<()> {
    let Base {
        schema,
        membership,
        parsed,
    } = base;
    // §6f: objects carry no locale, so an object view that declares them is a
    // config error rather than a silent ignore.
    if !parsed && v.locales.is_some() {
        bail!("view {name}: objects carry no locale; object views cannot declare locales");
    }
    // Parsed and type-checked once per view, not per row: a bad filter is a
    // startup error naming the view.
    let view = grackle_db::View::all()
        .filter(membership.and(scoped_filter(name, q, &schema)?))
        .order({
            // Sorting reads the same vocabulary the filter does — it used to
            // rebuild it here, which is how the two drifted apart in the first
            // place.
            let known: Vec<&str> = schema.keys().copied().collect();
            declared_order(&known, &format!("view {name}"), q.order_by.as_deref())?
        });
    let rows = &db.rows;

    // One row set per locale (§6f), the default included — it is not special.
    //
    // `rendered` and `!claimed` are no-ops on the posts side: every post is
    // parsed, and only a tree row can be a view's claimed content (q45).
    let rows_for = |locale: &str| -> Vec<grackle_db::Key> {
        let eligible: Vec<grackle_db::Key> = rows
            .iter()
            .filter(|p| !parsed || (p.rendered && !p.claimed && p.locale == locale))
            .map(|p| p.key.clone())
            .collect();
        rows.view_within(&eligible, &view)
    };

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
    // mirrored. An object view has one locale because an object has none.
    let locales = if parsed {
        locales_for(cfg, v)
    } else {
        vec![cfg.i18n.default.as_str()]
    };

    // Every template this view lands on. `path` and `paths` are one list here:
    // an unpaginated view uses the first, a paginated one needs the second for
    // its `{n}`, and reading them from one place is what lets a grouped view
    // paginate at all.
    let tmpls: Vec<String> = if v.routes.is_empty() {
        v.route.iter().cloned().collect()
    } else {
        v.routes.clone()
    };
    if tmpls.is_empty() {
        bail!("view {name} needs a route");
    }
    // Spend the axis segment before anything else reads the templates, so the
    // grouped, paginated and single-route branches below are unchanged: by the
    // time they see a template it is this member's.
    let tmpls: Vec<String> = match &member {
        Some(m) => tmpls
            .iter()
            .map(|t| t.replace(&format!("{{{}}}", m.axis), &m.value))
            .collect(),
        None => tmpls,
    };
    // Page 2 would otherwise reuse page 1's URL — a route collision reported
    // against two identical templates, which reads as a mystery. Every
    // paginated view in the corpus already declares both.
    if v.paginate.is_some() && tmpls.len() < 2 {
        bail!(
            "view {name} paginates but declares one path, so page 2 would reuse \
             page 1's URL. Give it two: `paths = [\"/x/\", \"/x/page/{{n}}/\"]`."
        );
    }

    // Grouped views, possibly a subdivision chain (§5c): a grouped view
    // `over` a grouped view refines the parent's partition, and the group keys
    // accumulate — GROUP BY year, month, expressed compositionally.
    if v.group_by.is_some() {
        let chain = cfg.group_specs(name);
        check_group_chain(name, &chain, &schema)?;
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
            let localized: Vec<String> =
                tmpls.iter().map(|t| prefixed(cfg, locale, t)).collect();
            let mut routes =
                grouped_routes(name, v, &member, &localized, &chain, &grouped, &route_value)?;
            for r in &mut routes {
                r.locale = stamp(cfg, locale);
            }
            db.routes.extend(routes);
        }
        return Ok(());
    }

    // Paginated list.
    if let Some(per) = v.paginate.map(|p| p.max(1)) {
        for locale in &locales {
            let row_ix = rows_for(locale);
            if row_ix.is_empty() && *locale != cfg.i18n.default {
                continue;
            }
            for n in 1..=row_ix.len().div_ceil(per) {
                let tmpl = if n == 1 { &tmpls[0] } else { &tmpls[1] };
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
                    fields: view_fields(v),
                    axis: member.clone(),
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
    let route = tmpls[0].as_str();
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
            fields: view_fields(v),
            axis: member.clone(),
            view: Some(name.to_string()),
            rows: Some(members.len()),
            members,
            locale: stamp(cfg, locale),
            ..Route::new(prefixed(cfg, locale, route), RouteKind::View)
        });
    }
    Ok(())
}

/// A view's declared filter, narrowed by its `match` chain.
///
/// The chain's globs are CONJOINED: a row must satisfy every one, so a child
/// narrows within its parent's subtree and can never widen out of it (§5c).
/// They compile to `glob(path, …)` rather than running as a separate pass,
/// which is what makes a scope and a filter one thing — composable, checked by
/// one type-checker, applied wherever the filter is.
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

/// Views over the whole route set (the sitemap). Runs after every other route
/// exists, and its `rows` is the count that actually passes its filter.
pub(crate) fn build_star_views(cfg: &Config, db: &mut SiteDb) -> Result<()> {
    for (name, v) in &cfg.views {
        if !v.over.is_star() {
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
/// A star view ranges over ROUTES, so its members name `db.routes` rather
/// than the row store — the one place in the engine where that is true, and
/// true because `over = "*"` says so.
///
/// Deferred to here because the route list is only final once it
/// stops growing and has been sorted. Resolving during `build_star_views`
/// measured a partial list: views build in name order, so `sitemap` saw
/// `search`'s route and would not have seen it the other way round. Both
/// filters happen to exclude the other's route by extension, which is why
/// nothing was visibly wrong.
pub(crate) fn resolve_star_views(cfg: &Config, db: &mut SiteDb, schemas: &Schemas) -> Result<()> {
    for (name, v) in &cfg.views {
        if !v.over.is_star() {
            continue;
        }
        let pred = match &v.filter {
            Some(src) => filter::Filter::parse(src, &route_schema(&schemas.declared_schema()))
                .with_context(|| format!("view {name}: filter {src:?}"))?,
            None => filter::Filter::always(),
        };
        let members = db.routes.select(&pred);
        // q53: a `*` view sees the CANONICAL member only. An axis publishes
        // alternative forms of one row, and an alternate is not a second page —
        // listing every member in the sitemap or the search index would be
        // asking a crawler to treat six renderings of one document as six
        // documents, which is the thing `rel="canonical"` exists to deny.
        // Reaching them is `rel="alternate"`'s job, in the head.
        let members: Vec<grackle_db::Key> = members
            .into_iter()
            .filter(|k| {
                db.routes
                    .get(k)
                    .is_none_or(|r| r.axis.as_ref().is_none_or(|a| a.canonical))
            })
            .collect();
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
// Membership, scoping and ordering moved to fixture tests (§7d): they
// hand-built `Row`s and wired `object_ix` themselves, so they could not have
// caught a bug in the loader they were imitating. See
// `crates/grackle/tests/fixtures/{object-gallery,post-ordering}`.
mod object_view_tests {
    use super::*;
    use grackle_model::Row;
    use std::path::PathBuf;

    /// An object row: a path, a URL, and nothing rendered. `name`, `ext` and
    /// `stem` all derive from `rel`, so there is nothing else to set.

    /// Seed the row store with object rows AND record their membership —
    /// without it the view's base filter matches nothing and the test passes
    /// vacuously.

    fn cfg(views: &str) -> Config {
        let src = format!(
            "root = \".\"\nextends = \"none\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
             [[collections]]\nname = \"objects\"\nkind = \"objects\"\n{views}"
        );
        Config::from_toml(&src).expect("test config parses")
    }

    /// With objects and content in ONE store, a gallery must see only
    /// objects. Membership is a filter; this test fails if it is dropped.

    /// The vocabulary is still the narrow one: an object cannot sort on a
    /// column only a content row has.
    #[test]
    fn an_object_view_sorts_only_on_object_columns() {
        let c = cfg("[routes.g]\nfrom = \"objects\"\norder_by = \"title\"\n\
             path = \"/p/\"\nlayout = \"listing\"\n");
        let e = format!(
            "{:#}",
            build_views(&c, &mut SiteDb::default(), &Schemas::new(row_schema())).unwrap_err()
        );
        assert!(e.contains("order_by names unknown field \"title\""), "{e}");
        // The object vocabulary is file facts only — including the pixel
        // shape, which is one.
        assert!(
            e.contains("dir, ext, height, name, path, size, stem, url, width"),
            "{e}"
        );
    }

    #[test]
    fn object_filters_typecheck_against_the_object_schema() {
        let c = cfg("[routes.g]\nfrom = \"objects\"\nwhere = \"draft\"\n\
             order_by = \"name\"\npath = \"/p/\"\nlayout = \"listing\"\n");
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
            // The view filters on locale, so a fixture row must carry the
            // default locale to be visible at all.
            locale: "en".into(),
            slug: url.trim_matches('/').into(),
            // A row's key is its path, so fixtures need distinct ones or
            // they are all the same row as far as the table is concerned.
            rel: std::path::PathBuf::from(format!("{}.md", url.trim_matches('/'))),
            // Every post is a rendered row; the loader hardcodes it, and
            // eligibility is a predicate, so the fixture must say so.
            rendered: true,
            ..Row::default()
        }
    }

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
            "root = \".\"\nextends = \"none\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
             [[collections]]\nname = \"notes\"\nkind = \"posts\"\nsource = \"_posts\"\n\
             filename_formats = [\"{{year}}-{{month}}-{{day}}-{{slug}}\"]\n\
             [routes.g]\nfrom = \"notes\"\npath = \"/g/\"\nlayout = \"listing\"\n{clauses}"
        );
        Config::from_toml(&src).expect("test config parses")
    }

    /// A view with no `order_by` orders by PATH — not newest-first. A posts
    /// collection asks for dates.

    /// `where` reads declared `.schema.toml` fields, like `order_by`,
    /// `group_by` and a relation's `rank` already did. It was the one consumer
    /// parsing against the bare row schema, so a site could declare a bool,
    /// group by it, sort by it — and then get `unknown field` from its own
    /// filter. Mutation-checked by restoring `row_schema()` at the parse site.
    #[test]
    fn a_where_may_name_a_declared_field() {
        use std::path::Path;
        let mut schemas = Schemas::new(row_schema());
        schemas
            .add(
                Path::new(""),
                "archived = { type = \"bool\" }\n",
                Path::new(".schema.toml"),
            )
            .unwrap();

        let clauses = "[sets.s]\nfrom = \"notes\"\nwhere = \"!archived\"\n";
        build_views(&cfg(clauses), &mut db(), &schemas)
            .expect("a declared field is nameable in `where`");

        // And it is still a load error when nothing declares it — the widening
        // is the declared set, not "anything goes".
        let e = build_views(&cfg(clauses), &mut db(), &Schemas::new(row_schema())).unwrap_err();
        // `{:#}` for the whole chain — the top frame is only "view s: filter …".
        assert!(format!("{e:#}").contains("unknown field"), "{e:#}");
    }

    /// `path` is the last key, always, so rows tied on the declared column
    /// order by their file rather than by whatever the walk yielded. Here
    /// `/b/` and `/c/` declare no `order`, tie at Null, and fall to path.

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
        // Through the real grouping path: comparing two `SortKey::Int`s
        // directly is true by `derive(Ord)` and would hold with months
        // grouped as strings, which is the bug — params carry an unpadded
        // "3", so lexical order puts December before March.
        let month = |d| key_combos(&post(Some(d), &[]), &["date.month".to_string()]);
        let (march, december) = (month("2022-03-16"), month("2022-12-16"));
        assert!(
            march[0][0].sort < december[0][0].sort,
            "March must sort before December"
        );
        // Display zero-pads narrow values and leaves a year alone.
        assert_eq!(march[0][0].sort.display(), "03");
        assert_eq!(
            key_combos(&post(Some("2022-03-16"), &[]), &["date.year".to_string()])[0][0]
                .sort
                .display(),
            "2022"
        );
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

        // `date.year` over a PAGE: grouping does not care which origin a row
        // came from. An undated page is absent from the year partition,
        // exactly as an undated post is.
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

/// What `next`/`previous` step through (q51). The collection anchor is
/// structural — one sequence per collection, not a filter after the fact.
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
            // A flag is a declared field now (§4e), so a fixture sets one the
            // way a row carries one.
            fields: BTreeMap::from([("draft".to_string(), grackle_db::Value::Bool(draft))]),
            locale: "en".into(),
            ..Row::default()
        }
    }

    fn db_with(rows: Vec<Row>) -> SiteDb {
        SiteDb::seed(rows, true)
    }

    fn cfg(extra: &str) -> Config {
        Config::from_toml(&format!(
            "root = \".\"\nextends = \"none\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
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

    /// Two dated collections interleave in one table, so a shared index made
    /// a blog post's neighbour a note. One sequence per collection makes that
    /// unable to recur.
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

    /// The diagnostic walk is a raw chronological table order — every dated
    /// row of the collection, newest first, drafts included. Selecting which
    /// rows are neighbours on a rendered page is the relation engine's job now
    /// (§6g), not this sequence's; `grackle explain` only reports the table.
    #[test]
    fn the_diagnostic_walk_is_chronological_and_unfiltered() {
        let mut db = db_with(vec![
            post("posts", "/blog/jan/", Some("2026-01-01"), false),
            post("posts", "/blog/feb/", Some("2026-02-01"), true), // dated DRAFT
            post("posts", "/blog/mar/", Some("2026-03-01"), false),
        ]);
        build_adjacency(&cfg(""), &mut db, &Schemas::new(row_schema())).unwrap();
        assert_eq!(
            seq(&db, "posts"),
            ["/blog/mar/", "/blog/feb/", "/blog/jan/"],
            "the raw table walk includes a dated draft — the relation engine, \
             not this sequence, is what excludes it from a page"
        );
    }
}

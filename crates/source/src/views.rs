//! Views become routes: resolving each declared query into row sets, group
//! partitions (subdivision, §5c) and materialized `Route`s. Split from the
//! table-building half of the database (`db.rs`); `SiteDb::load` calls in
//! here once the tables and row routes exist.

use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;

use crate::config::{Config, Query, View};
use crate::schema::Schemas;
use grackle_db::filter;
use grackle_db::template;
use grackle_model::{route_schema, row_schema, AxisMember, Route, RouteKind, SiteDb, ViewRows};

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

/// One cell of a view's route product: the rows that landed in it, the URL
/// params that name it, and the key it wears.
///
/// A view without a `group_by` has exactly one cell holding everything, which
/// is what lets the materializer below have no branches — an ungrouped view is
/// a grouped one with a single partition, and a single-page view is a
/// paginated one whose slice happens to be the whole cell.
struct Cell {
    key: Option<String>,
    params: Vec<(String, String)>,
    rows: Vec<grackle_db::Key>,
}

/// Partition rows into cells by a subdivision chain (§5c) — one cell per
/// composite group key, in key order. Shared by every base table: grouping
/// never cared what a post or a page was.
fn partition(chain: &[String], rows: &[(grackle_db::Key, &dyn filter::Row)]) -> Vec<Cell> {
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
    groups
        .into_iter()
        .map(|(sort, (params, rows))| Cell {
            key: Some(
                sort.iter()
                    .map(SortKey::display)
                    .collect::<Vec<_>>()
                    .join("-"),
            ),
            params,
            rows,
        })
        .collect()
}

/// Which locales a materializing view partitions into (§6f). Default-on:
/// every locale, unless the view opts out with `locales = "default"`.
fn locales_for<'a>(cfg: &'a Config, v: &View) -> Vec<&'a str> {
    match v.locales.as_deref() {
        Some("default") => vec![cfg.i18n.default.as_str()],
        _ => cfg.locales(),
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
) {
    db.views.insert(
        name.to_string(),
        ViewRows {
            layout: v.layout.clone(),
            variant: v.variant.clone(),
            rows: members.len(),
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
    for cname in cfg.collections.keys() {
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
/// Schema-declared values on the view (`noindex = true`, …) land on the
/// route under the same names a row would use, so `[html.head.meta]` and
/// `where` share one vocabulary. `shell` is always present: absent means
/// the HTML listing, and fold filters need a total column.
fn view_fields(
    v: &View,
    schema: &BTreeMap<String, crate::schema::FieldType>,
) -> anyhow::Result<BTreeMap<String, filter::Value>> {
    let mut f = BTreeMap::new();
    for (name, raw) in &v.route_fields {
        let ty = schema.get(name.as_str()).copied().with_context(|| {
            format!("view route field {name:?} was not validated against [schema]")
        })?;
        f.insert(
            name.clone(),
            crate::schema::typed(ty, name, raw, "view")?,
        );
    }
    // IO.md §3: `shell` is "the serialization it left through", a fact about
    // the OUTPUT — so a view route answers the same column a row route does,
    // out of the same map. Before I2 the declaration was a route property the
    // column could not see, and `shell == "atom"` selected nothing on a site
    // with a feed.
    //
    // Absent means the HTML listing, and it is written here rather than left
    // Null for the reason I1 refused to write it on rows and I2 does: a fold
    // over the route pool needs a total predicate, and "the shell it left
    // through" is answerable for every route in the table.
    f.insert(
        "shell".to_string(),
        filter::Value::Str(
            v.shell
                .clone()
                .unwrap_or_else(|| crate::shell::VIEW_DEFAULT.to_string()),
        ),
    );
    Ok(f)
}

pub(crate) fn build_views(cfg: &Config, db: &mut SiteDb, schemas: &Schemas) -> Result<()> {
    let route_schema = crate::schema::site_fields(&cfg.schema, "grackle.toml [schema]")?;
    for (name, v) in &cfg.views {
        // A fold with no `from` reads every output (IO.md §4) — at this stage
        // the finished route set — so it runs in a second pass (see
        // `build_pool_folds`). Views iterate in name order, so inline would
        // measure a partial list.
        if v.reads_all_outputs() {
            continue;
        }
        // Both named queries (`published`) and embedded views (`latest`) still
        // have to resolve, so a typo in `from` is a startup error either way.
        let q = cfg.query(name)?;
        // The base's role still decides row eligibility (objects skip it). A
        // union's members share a role — they share a `from` vocabulary — so
        // the first collection answers for the whole base.
        let base_is_objects = q
            .base
            .first()
            .and_then(|n| cfg.collections.get(n))
            .is_some_and(|c| c.is_objects());
        let base = Base::resolve(schemas, name, &q, base_is_objects)?;
        // q53: the axis is the OUTERMOST dimension — a view on one materializes
        // once per member, and everything below it (locale, grouping,
        // pagination) happens within each. Ordering it outermost is what keeps
        // the branches below a substitution rather than a rewrite.
        for members in axis_member_combos(cfg, name, v)? {
            build_view(cfg, db, name, v, &q, base.clone(), members, &route_schema)?;
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
        !v.inherited || v.reads_all_outputs() || r.rows.is_none_or(|n| n > 0)
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
    fn resolve(schemas: &Schemas, name: &str, q: &Query, is_objects: bool) -> Result<Base> {
        // One rule for every base: the row's collection is one of the ones
        // `from` named. One row schema answers every view (IO.md §3): an object
        // is a `Row` like any other, so `where` on a gallery sees the same
        // columns — path, dir, name, ext, size, width, height all live on the
        // row schema — that a post's `where` sees.
        let membership = filter::Filter::parse(&members_clause(&q.base), &row_schema())
            .with_context(|| format!("view {name}: base {:?}", q.base))?;
        Ok(Base {
            // Built-ins plus every declared field, so `where` sees exactly
            // what `order_by`, `group_by` and a relation's `rank` see.
            schema: schemas.row_filter_schema(),
            membership,
            // Objects are never rendered, never a view's claimed content and
            // carry no locale, so row eligibility would exclude every one of
            // them — the one place the base's role still bends the flow.
            parsed: !is_objects,
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

/// The member-tuples a view materializes across — the cartesian product of its
/// declared axes, or a single empty tuple for a view on no axis, so the caller
/// loops either way and there is no second path for the ordinary case. A view
/// on one axis yields one member per value.
fn axis_member_combos(cfg: &Config, name: &str, v: &View) -> Result<Vec<Vec<AxisMember>>> {
    let mut combos: Vec<Vec<AxisMember>> = vec![Vec::new()];
    for axis_name in &v.axis {
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
        // Each axis must be spent somewhere in the path, or its members collide
        // on one URL and all but one are lost. Checked here because this is
        // where the two halves — the axis and the path that allocates it — are
        // both in hand, and per axis because a product must spend every one.
        //
        // Asked through `load::spends`, the same predicate `select_path` fills
        // by: this check used to carry its own `{name}` string, so a path
        // written the namespaced way (`{axis:theme}`) failed here while the
        // materializer would have spent it happily (MERGE.md C5).
        if !v
            .route
            .iter()
            .chain(v.routes.iter())
            .any(|t| crate::load::spends(t, axis_name))
        {
            bail!(
                "view {name}: axis = {axis_name:?} but no path spends it — give the \
                 path a {{{axis_name}}} (or {{axis:{axis_name}}}) segment, or the \
                 members would collide on one URL"
            );
        }
        combos = combos
            .into_iter()
            .flat_map(|c| {
                axis.values.iter().map(move |value| {
                    let mut c2 = c.clone();
                    c2.push(AxisMember {
                        axis: axis_name.to_string(),
                        value: value.clone(),
                        field: axis.field.clone(),
                        canonical: axis.canonical() == Some(value.as_str()),
                    });
                    c2
                })
            })
            .collect();
    }
    Ok(combos)
}

/// Materialize a view — one flow for every base.
///
/// A view differs from another only in which index list it starts from: a set,
/// not a shape. Ordering, `limit`, grouping, pagination and the locale axis
/// are one rule each, applied here for every base.
fn build_view(
    cfg: &Config,
    db: &mut SiteDb,
    name: &str,
    v: &View,
    q: &Query,
    base: Base,
    axis_members: Vec<AxisMember>,
    route_schema: &BTreeMap<String, crate::schema::FieldType>,
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
        .filter(membership.and(declared_filter(name, q, &schema)?))
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
        insert_routeless(db, name, v, members);
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
    // The route templates split by pagination: those WITHOUT `{n}` are page-1 /
    // default-variant candidates, those WITH it paginate. Axis and locale are no
    // longer spent up front — each cell selects among the candidates by which
    // non-canonical axes it must show (§6f, the default-axis case), so a member
    // at its canonical value can drop its segment to a shorter template.
    let page1: Vec<&String> = tmpls.iter().filter(|t| !t.contains("{n}")).collect();
    let paged: Vec<&String> = tmpls.iter().filter(|t| t.contains("{n}")).collect();
    if v.paginate.is_some() && paged.is_empty() {
        bail!(
            "view {name} paginates but no path spends {{n}}, so page 2 would reuse \
             page 1's URL. Give it one: `paths = [\"/x/\", \"/x/page/{{n}}/\"]`."
        );
    }
    if page1.is_empty() {
        bail!("view {name} needs a path with no {{n}} for page one");
    }

    // ONE materialization, over the product of the view's dimensions.
    //
    // These used to be three branches — grouped, paginated, single — each with
    // its own loop over locales and its own idea of how a route is built, and
    // the grouped one carried a second copy of the pagination loop. They are
    // one shape: partition, slice, emit. An ungrouped view is a grouped one
    // with a single cell; a single-page view is a paginated one whose slice is
    // the whole cell.
    //
    // The dimensions, outermost first: AXIS (spent by the caller, already in
    // `tmpls`), LOCALE, GROUP, then PAGE slicing whatever cell it is handed.
    let chain = cfg.group_specs(name);
    if !chain.is_empty() {
        check_group_chain(name, &chain, &schema)?;
    }
    // §6f enum records: URLs wear the record's slug for ANY grouped field
    // (tags, courses, …); keys and titles keep the id.
    let leaf = chain
        .last()
        .map(|s| grackle_model::spec_field(s).to_string());
    let route_value = |k: &str, val: &str| -> String {
        let field = if k == "key" { leaf.as_deref() } else { Some(k) };
        match field {
            Some(f) => cfg.record_slug(f, val).to_string(),
            None => val.to_string(),
        }
    };

    for locale in &locales {
        let row_ix = rows_for(locale);
        let cells = if chain.is_empty() {
            // No rows in this locale = no page (the partition is real, §6f).
            // A GROUPED view needs no such rule: a group with no rows is a
            // group that does not exist.
            if row_ix.is_empty() && *locale != cfg.i18n.default {
                continue;
            }
            vec![Cell {
                key: None,
                params: Vec::new(),
                rows: row_ix,
            }]
        } else {
            let rows_ref: Vec<(grackle_db::Key, &dyn filter::Row)> = row_ix
                .iter()
                .filter_map(|k| rows.get(k).map(|r| (k.clone(), r as &dyn filter::Row)))
                .collect();
            partition(&chain, &rows_ref)
        };

        for cell in cells {
            // Render a candidate: group params and `{n}` filled, axis and locale
            // placeholders PRESERVED for `select_path` to spend per member.
            let render = |tmpl: &str, n: Option<usize>| -> Result<String> {
                template::render(tmpl, |tok| {
                    let (ns, k) = template::classify(tok);
                    match ns {
                        None if k == "n" => n.map(|n| n.to_string()),
                        // Preserve an axis or locale token — it is spent by the
                        // selection below, not filled from group params here.
                        Some("axis") => Some(format!("{{{tok}}}")),
                        None if cfg.axes.contains_key(k) => Some(format!("{{{k}}}")),
                        None if k == "locale" && cfg.locale_enabled() => Some("{locale}".to_string()),
                        None | Some("group") => {
                            template::param(&cell.params, k).map(|val| route_value(k, &val))
                        }
                        _ => None,
                    }
                })
            };
            // This cell's coordinates: the axis member-tuple, plus the row set's
            // locale. `select_path` picks the shortest template covering the
            // non-canonical ones.
            let coords: Vec<crate::load::Coord> = axis_members
                .iter()
                .map(|m| crate::load::Coord {
                    axis: &m.axis,
                    value: &m.value,
                    canonical: m.canonical,
                })
                .chain(std::iter::once(crate::load::Coord {
                    axis: "locale",
                    value: locale,
                    canonical: *locale == cfg.i18n.default,
                }))
                .collect();
            let pick = |cands: &[&String], n: Option<usize>| -> Result<String> {
                let rendered: Vec<String> =
                    cands.iter().map(|t| render(t, n)).collect::<Result<_>>()?;
                crate::load::select_path(&rendered, &coords)
            };
            match v.paginate.map(|p| p.max(1)) {
                Some(per) => {
                    for n in 1..=cell.rows.len().div_ceil(per).max(1) {
                        let url = if n == 1 {
                            pick(&page1, None)?
                        } else {
                            pick(&paged, Some(n))?
                        };
                        let page: Vec<grackle_db::Key> = cell
                            .rows
                            .iter()
                            .skip(per * (n - 1))
                            .take(per)
                            .cloned()
                            .collect();
                        db.routes.push(Route {
                            fields: view_fields(v, route_schema)?,
                            axis: axis_members.clone(),
                            view: Some(name.to_string()),
                            // A grouped page keeps its GROUP key — `{key}` in a
                            // title names the partition — and `page` carries the
                            // number. Ungrouped, the page number is all there is.
                            key: cell.key.clone().or_else(|| Some(format!("page {n}"))),
                            rows: Some(page.len()),
                            page: Some(n),
                            params: cell.params.clone(),
                            members: page,
                            locale: stamp(cfg, locale),
                            ..Route::new(url, RouteKind::View)
                        });
                    }
                }
                None => {
                    // `limit` truncates only an unpaginated, ungrouped view —
                    // where it means "the feed's twenty". Preserved as-is rather
                    // than generalized: a limit over a partition would silently
                    // start truncating group pages.
                    let members: Vec<grackle_db::Key> = match chain.is_empty() {
                        true => cell
                            .rows
                            .iter()
                            .take(v.limit.unwrap_or(cell.rows.len()))
                            .cloned()
                            .collect(),
                        false => cell.rows.clone(),
                    };
                    db.routes.push(Route {
                        fields: view_fields(v, route_schema)?,
                        axis: axis_members.clone(),
                        view: Some(name.to_string()),
                        key: cell.key.clone(),
                        rows: Some(members.len()),
                        params: cell.params.clone(),
                        members,
                        locale: stamp(cfg, locale),
                        ..Route::new(pick(&page1, None)?, RouteKind::View)
                    });
                }
            }
        }
    }
    Ok(())
}

/// A view's declared filter: the conjunction of every `where` along its
/// `from` chain.
///
/// Path scoping is in there, as `glob(path, …)` clauses an author writes
/// (MERGE.md G2 retired the `match` key that used to compile to exactly
/// that). A scope and a filter are one thing — composable, checked by one
/// type-checker, applied wherever the filter is — so there is one conjunction
/// here rather than a predicate and a glob pass beside it.
fn declared_filter(name: &str, q: &Query, schema: &filter::Schema) -> Result<filter::Filter> {
    Ok(match q.predicate() {
        // A profile's `where` is type-checked HERE, by the pass that evaluates
        // it — `Config` alone cannot see the positional `.schema.toml`
        // vocabulary, so refusing an unknown name at profile-apply time would
        // make a profile's filter stricter than the one it replaces (§4a,
        // MERGE.md C6a). The source parsed here is the whole `from` chain, so
        // the note is what keeps the profile in the message.
        Some(src) => filter::Filter::parse(&src, schema).with_context(|| {
            let note = match q.patched.is_empty() {
                true => String::new(),
                false => format!(" ({})", q.patched.join("; ")),
            };
            format!("view {name}: filter {src:?}{note}")
        })?,
        None => filter::Filter::always(),
    })
}

/// Folds over every output — a fold shell with no `from` (IO.md §4): the
/// sitemap, the search index. Runs after every other route exists, and its
/// `rows` is the count that actually passes its filter.
///
/// "Every output" is the route set at this stage of the migration, which is
/// the outputs database's facts half and is exactly what the retired
/// `from = "*"` read. A fold whose `from` names a *set* is not here — it goes
/// through `build_views` with its rows, and the join makes that selection
/// output-mediated at I9.
pub(crate) fn build_pool_folds(cfg: &Config, db: &mut SiteDb) -> Result<()> {
    let route_schema = crate::schema::site_fields(&cfg.schema, "grackle.toml [schema]")?;
    for (name, v) in &cfg.views {
        if !v.reads_all_outputs() {
            continue;
        }
        let tmpl = v
            .route
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("view {name} needs a route"))?;
        db.routes.push(Route {
            view: Some(name.clone()),
            // This fold answers the `shell` column like any other route
            // (I2): `shell == "sitemap"` selects the route the sitemap leaves
            // through. Route fields (`noindex`, …) ride along from the view.
            fields: view_fields(v, &route_schema)?,
            ..Route::new(tmpl.to_string(), RouteKind::View)
        });
    }
    Ok(())
}

/// Resolve each all-outputs fold's members, once the route list is final.
///
/// These range over ROUTES, so their members name `db.routes` rather than the
/// row store — the one place in the engine where that is true, and true
/// because the absent `from` says so (IO.md §4).
///
/// Deferred to here because the route list is only final once it
/// stops growing and has been sorted. Resolving during `build_pool_folds`
/// measured a partial list: views build in name order, so `sitemap` saw
/// `search`'s route and would not have seen it the other way round. Both
/// filters happen to exclude the other's route by extension, which is why
/// nothing was visibly wrong.
pub(crate) fn resolve_pool_folds(cfg: &Config, db: &mut SiteDb, schemas: &Schemas) -> Result<()> {
    for (name, v) in &cfg.views {
        if !v.reads_all_outputs() {
            continue;
        }
        let pred = match &v.filter {
            Some(src) => filter::Filter::parse(src, &route_schema(&schemas.declared_schema()))
                .with_context(|| match &v.filter_profile {
                    // As in `declared_filter`: a fold's `where` may have been
                    // replaced by a profile, and the message says so.
                    Some(p) => {
                        format!("view {name}: filter {src:?} (profile {p} replaced its `where`)")
                    }
                    None => format!("view {name}: filter {src:?}"),
                })?,
            None => filter::Filter::always(),
        };
        let members = db.routes.select(&pred);
        // q53: an all-outputs fold sees the CANONICAL member only. An axis publishes
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
                    .is_none_or(|r| r.axis.iter().all(|a| a.canonical))
            })
            .collect();
        let Some(at) = db
            .routes
            .iter()
            // The `view` column alone: a route naming this view IS a view
            // route, so the `kind == View` term that stood here was saying it
            // twice (IO.md §3, I13).
            .find(|r| r.view.as_deref() == Some(name.as_str()))
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

    fn cfg(views: &str) -> Config {
        let src = format!(
            "root = \".\"\nextends = \"none\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
             [[collections]]\nname = \"objects\"\n{views}"
        );
        Config::from_toml(&src).expect("test config parses")
    }

    /// G2's precondition: a gallery scopes itself with `glob(path, …)` in its
    /// `where`, and that type-checks HERE — `path` is a column of the one row
    /// schema every view sees now (IO.md §3, an object is a `Row` like any
    /// other), so retiring the `match` key cost object views nothing.
    #[test]
    fn an_object_view_scopes_itself_with_a_path_glob() {
        let c = cfg("[routes.g]\nfrom = \"objects\"\n\
             where = 'glob(path, \"photos/**\")'\n\
             order_by = \"name\"\npath = \"/p/\"\nlayout = \"card\"\n");
        build_views(&c, &mut SiteDb::default(), &Schemas::new(row_schema()))
            .expect("a path glob is object vocabulary");
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
             [[collections]]\nname = \"notes\"\nsource = \"_posts\"\n\
             file = [\"{{year}}-{{month}}-{{day}}-{{slug}}\"]\n\
             [routes.g]\nfrom = \"notes\"\npath = \"/g/\"\nlayout = \"card\"\n{clauses}"
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
            description: None,
            order: None,
            date: None,
            tags: Vec::new(),
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
             [[collections]]\nname = \"posts\"\nsource = \"_posts\"\n\
             file = [\"{{year}}-{{month}}-{{day}}-{{slug}}\"]\n{extra}"
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
        let c = cfg("[[collections]]\nname = \"notes\"\n\
                     source = \"_notes\"\nfile = [\"{year}-{month}-{day}-{slug}\"]\n");
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

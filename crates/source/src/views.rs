//! Views become routes: resolve queries into row sets, partitions (§5c), and `Route`s.

use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;

use crate::config::{Config, Query, View};
use crate::schema::Schemas;
use grackle_db::filter;
use grackle_db::template;
use grackle_model::{route_schema, row_schema, AxisMember, Route, RouteKind, SiteDb, ViewRows};

/// One `group_by` key: sort component, display (`Route.key`), and template params.
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
            // Zero-pad narrow numerics so `2022-3` reads `2022-03`.
            SortKey::Int(n) if *n < 10 => format!("0{n}"),
            SortKey::Int(n) => n.to_string(),
            SortKey::Str(s) => s.clone(),
        }
    }
}

/// Keys under one `group_by` spec: List multi-keys, scalars single-key, Null absent.
/// Also exposes `{year}`/`{month}`/`{day}` for `date.*` specs.
fn group_keys(row: &dyn filter::Row, spec: &str) -> Vec<GroupKey> {
    let mk = |sort: SortKey, display: String| {
        let mut params = vec![("key".to_string(), display.clone())];
        if spec != "key" {
            params.push((spec.to_string(), display.clone()));
        }
        if let Some((_, part)) = spec.rsplit_once('.') {
            if matches!(part, "year" | "month" | "day") {
                params.push((part.to_string(), display.clone()));
            }
        }
        GroupKey { sort, params }
    };
    match row.field(spec) {
        filter::Value::List(items) => items
            .into_iter()
            .filter_map(|v| match v {
                filter::Value::Str(t) => Some(mk(SortKey::Str(t.clone()), t)),
                _ => None,
            })
            .collect(),
        filter::Value::Str(s) => vec![mk(SortKey::Str(s.clone()), s)],
        filter::Value::Int(i) => vec![mk(SortKey::Int(i), i.to_string())],
        // Doubles are expression-only; string-form rather than drop if hit.
        filter::Value::Double(d) => vec![mk(SortKey::Str(d.to_string()), d.to_string())],
        filter::Value::Bool(b) => vec![mk(SortKey::Str(b.to_string()), b.to_string())],
        filter::Value::Content(_)
        | filter::Value::Outline(_)
        | filter::Value::Map(_)
        | filter::Value::Null => Vec::new(),
    }
}

/// Every `group_by` spec must name a field of the base's vocabulary (§5c).
fn check_group_chain(name: &str, chain: &[String], schema: &filter::Schema) -> Result<()> {
    let mut known: Vec<&str> = schema.keys().copied().collect();
    known.sort_unstable();
    known.dedup();
    for spec in chain {
        if !known.contains(&spec.as_str()) {
            bail!(
                "view {name}: group_by names unknown field {spec:?}\n  known fields: {}",
                known.join(", ")
            );
        }
    }
    Ok(())
}

/// Cartesian product of group keys across a subdivision chain; empty if absent at any level.
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

/// One partition cell: rows, URL params, and display key.
struct Cell {
    key: Option<String>,
    params: Vec<(String, String)>,
    rows: Vec<grackle_db::Key>,
}

/// Partition rows by subdivision chain (§5c), one cell per composite key.
fn partition(chain: &[String], rows: &[(grackle_db::Key, &dyn filter::Row)]) -> Vec<Cell> {
    #[allow(clippy::type_complexity)]
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

/// Pairing-axis values to materialize (§6f). Default-on; `partition = "default"` = canonical only.
fn partition_values<'a>(cfg: &'a Config, v: &View) -> Vec<&'a str> {
    match (v.partition.as_deref(), cfg.pairing_axis()) {
        (_, None) => vec![""],
        (Some("default"), Some((_, axis))) => {
            vec![axis.canonical().unwrap_or("")]
        }
        (_, Some((_, axis))) => axis.values.iter().map(String::as_str).collect(),
    }
}

fn view_fields_at(
    v: &View,
    route_schema: &BTreeMap<String, crate::schema::FieldType>,
    cfg: &Config,
    axis_value: &str,
) -> anyhow::Result<BTreeMap<String, filter::Value>> {
    let mut fields = view_fields(v, route_schema)?;
    if let Some((name, _)) = cfg.pairing_axis() {
        cfg.stamp_axis_field(&mut fields, name, axis_value);
    }
    Ok(fields)
}

fn insert_routeless(db: &mut SiteDb, name: &str, v: &View, members: Vec<grackle_db::Key>) {
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

/// Newest first, undated last, slug tiebreak (q51). First declared date field.
pub fn chronological(
    declared: &grackle_db::filter::Schema,
    rows: &[grackle_model::Row],
    a: usize,
    b: usize,
) -> std::cmp::Ordering {
    let field = grackle_model::date_fields(declared).into_iter().next();
    let (x, y) = (&rows[a], &rows[b]);
    let xd = field.and_then(|f| x.as_date(f));
    let yd = field.and_then(|f| y.as_date(f));
    match (xd, yd) {
        (Some(p), Some(q)) => q.cmp(&p),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
    .then_with(|| x.slug.cmp(&y.slug))
}

/// Sequence default: newest first, undated last, path ties. Not a view's default.
fn newest_first(declared: &grackle_db::filter::Schema) -> Vec<grackle_db::Order> {
    let mut orders = Vec::new();
    if let Some(f) = grackle_model::date_fields(declared).into_iter().next() {
        orders.push(grackle_db::Order::desc(f));
    }
    orders.push(grackle_db::Order::asc("path"));
    orders
}

/// View `order_by` plus a final `path` tiebreak. Default is path alone.
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

/// CLI `grackle explain` chronological walk (canonical only, §6f). Rendered
/// earlier/later steps are relations (§6g), not this.
pub(crate) fn build_adjacency(cfg: &Config, db: &mut SiteDb, schemas: &Schemas) -> Result<()> {
    let mut out: BTreeMap<String, Vec<grackle_db::Key>> = BTreeMap::new();
    let order = newest_first(&schemas.declared_schema());
    for cname in cfg.collections.keys() {
        let ix: Vec<grackle_db::Key> = db
            .rows
            .iter()
            .filter(|p| p.collection == *cname)
            .filter(|p| match cfg.pairing_axis() {
                Some((name, _)) => cfg.on_canonical(*p, name),
                None => true,
            })
            .map(|p| p.key.clone())
            .collect();
        let seq = grackle_db::View::all().order(order.clone());
        out.insert(cname.clone(), db.rows.view_within(&ix, &seq));
    }
    db.adjacency = out;
    Ok(())
}

/// View declarations as route fields (§4e). `shell` always set (fold filters need it).
fn view_fields(
    v: &View,
    schema: &BTreeMap<String, crate::schema::FieldType>,
) -> anyhow::Result<BTreeMap<String, filter::Value>> {
    let mut f = BTreeMap::new();
    for (name, raw) in &v.route_fields {
        let ty = schema.get(name.as_str()).cloned().with_context(|| {
            format!("view route field {name:?} was not validated against [schema]")
        })?;
        f.insert(name.clone(), crate::schema::typed(&ty, name, raw, "view")?);
    }
    // IO.md §3: shell is an output column; absent = HTML listing (fold filters).
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
    let route_schema = crate::schema::site_fields(&cfg.schema.decls, "grackle.toml [schema]")?;
    for (name, v) in &cfg.views {
        // No-`from` folds run later (`build_pool_folds`); inline would see a partial set.
        if v.reads_all_outputs() {
            continue;
        }
        let q = cfg.query(name)?;
        // Objects skip row eligibility; union members share a role via first collection.
        let base_is_objects = q
            .base
            .first()
            .and_then(|n| cfg.collections.get(n))
            .is_some_and(|c| c.is_objects());
        let base = Base::resolve(schemas, name, &q, base_is_objects)?;
        // q53: axis outermost so i18n/group/page stay a substitution within each member.
        for members in axis_member_combos(cfg, name, v)? {
            build_view(cfg, db, name, v, &q, base.clone(), members, &route_schema)?;
        }
    }
    // §4d: inherited empty routes do not materialize; site-declared ones may.
    db.routes.retain(|r| {
        let Some(v) = r.view.as_deref().and_then(|n| cfg.views.get(n)) else {
            return true; // a row's own route, not a view's
        };
        !v.inherited || v.reads_all_outputs() || r.rows.is_none_or(|n| n > 0)
    });
    Ok(())
}

/// Materialization inputs from a view's base: schema, membership, parsed-vs-object.
#[derive(Clone)]
struct Base {
    schema: filter::Schema,
    membership: filter::Filter,
    /// False for objects: row eligibility (`rendered`/`claimed`/i18n) would exclude all.
    parsed: bool,
}

impl Base {
    fn resolve(schemas: &Schemas, name: &str, q: &Query, is_objects: bool) -> Result<Base> {
        // Membership = collections `from` named. One row schema for every view (IO.md §3).
        let membership = filter::Filter::parse(&members_clause(&q.base), &row_schema())
            .with_context(|| format!("view {name}: base {:?}", q.base))?;
        Ok(Base {
            schema: schemas.row_filter_schema(),
            membership,
            parsed: !is_objects,
        })
    }
}

/// `collection == a || ...` over `from`; empty means nothing, not everything.
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

/// Cartesian product of declared axes (or one empty tuple if none).
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
        // Every axis must be spent in a path, else members collide (MERGE.md C5).
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

#[allow(clippy::too_many_arguments)]
/// Materialize a view: one flow for every base.
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
    // §6f: objects have no pairing axis.
    if !parsed && v.partition.is_some() {
        bail!("view {name}: objects have no pairing axis; object views cannot declare partition");
    }
    // Filter once per view (startup error), same schema as order_by.
    let view = grackle_db::View::all()
        .filter(membership.and(declared_filter(name, q, &schema)?))
        .order({
            let known: Vec<&str> = schema.keys().copied().collect();
            declared_order(&known, &format!("view {name}"), q.order_by.as_deref())?
        });
    let rows = &db.rows;

    // One row set per pairing-axis value (§6f); single cell if none.
    let pairing = cfg.pairing_axis();
    let rows_for = |axis_value: &str| -> Vec<grackle_db::Key> {
        let eligible: Vec<grackle_db::Key> = rows
            .iter()
            .filter(|p| {
                !parsed
                    || (p.rendered
                        && !p.claimed
                        && match pairing {
                            Some((_, axis)) => p.string(&axis.field) == Some(axis_value),
                            None => true,
                        })
            })
            .map(|p| p.key.clone())
            .collect();
        rows.view_within(&eligible, &view)
    };

    if !v.is_materialized() {
        let canon = pairing.and_then(|(_, a)| a.canonical()).unwrap_or("");
        let members: Vec<grackle_db::Key> = rows_for(canon)
            .into_iter()
            .take(v.limit.unwrap_or(usize::MAX))
            .collect();
        insert_routeless(db, name, v, members);
        return Ok(());
    }

    // Pairing partition default-on; `partition = "default"` opts out.
    let axis_values = if parsed {
        partition_values(cfg, v)
    } else {
        vec![""]
    };

    // `path` and `paths` as one list so grouped views can paginate.
    let tmpls: Vec<String> = if v.routes.is_empty() {
        v.route.iter().cloned().collect()
    } else {
        v.routes.clone()
    };
    if tmpls.is_empty() {
        bail!("view {name} needs a route");
    }
    // Without `{n}` = page-1 candidates; with `{n}` paginate. Axes spent at select (§6f).
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

    // Dimensions outermost-first: axis, locale, group, page.
    let chain = cfg.group_specs(name);
    if !chain.is_empty() {
        check_group_chain(name, &chain, &schema)?;
    }
    // §6f: grouped URLs wear record slug; keys/titles keep the id.
    let leaf = chain.last().cloned();
    let route_value = |k: &str, val: &str| -> String {
        let field = if k == "key" { leaf.as_deref() } else { Some(k) };
        match field {
            Some(f) => cfg.record_slug(f, val).to_string(),
            None => val.to_string(),
        }
    };

    let pairing = cfg.pairing_axis();
    let pairing_canon = pairing.and_then(|(_, a)| a.canonical()).unwrap_or("");
    for axis_value in &axis_values {
        let row_ix = rows_for(axis_value);
        let cells = if chain.is_empty() {
            // Empty non-canonical pairing cell: no page (§6f).
            if row_ix.is_empty() && *axis_value != pairing_canon {
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
            // Fill group/`{n}`; leave axis tokens for `select_path`.
            let render = |tmpl: &str, n: Option<usize>| -> Result<String> {
                template::render(tmpl, |tok| {
                    let (ns, k) = template::classify(tok);
                    match ns {
                        None if k == "n" => n.map(|n| n.to_string()),
                        Some("axis") => Some(format!("{{{tok}}}")),
                        None if cfg.axes.contains_key(k) => Some(format!("{{{k}}}")),
                        None | Some("group") => {
                            template::param(&cell.params, k).map(|val| route_value(k, &val))
                        }
                        _ => None,
                    }
                })
            };
            // Axes to spend; `select_path` picks the shortest covering template.
            let mut coords: Vec<crate::load::Coord> = axis_members
                .iter()
                .map(|m| crate::load::Coord {
                    axis: &m.axis,
                    value: &m.value,
                    canonical: m.canonical,
                })
                .collect();
            if let Some((axis_name, _)) = pairing {
                if page1
                    .iter()
                    .chain(paged.iter())
                    .any(|t| crate::load::spends(t, axis_name))
                {
                    coords.push(crate::load::Coord {
                        axis: axis_name,
                        value: axis_value,
                        canonical: *axis_value == pairing_canon,
                    });
                }
            }
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
                            fields: view_fields_at(v, route_schema, cfg, axis_value)?,
                            axis: axis_members.clone(),
                            view: Some(name.to_string()),
                            key: cell.key.clone().or_else(|| Some(format!("page {n}"))),
                            rows: Some(page.len()),
                            page: Some(n),
                            params: cell.params.clone(),
                            members: page,
                            ..Route::new(url, RouteKind::View)
                        });
                    }
                }
                None => {
                    // `limit` only on unpaginated ungrouped views (feed size).
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
                        fields: view_fields_at(v, route_schema, cfg, axis_value)?,
                        axis: axis_members.clone(),
                        view: Some(name.to_string()),
                        key: cell.key.clone(),
                        rows: Some(members.len()),
                        params: cell.params.clone(),
                        members,
                        ..Route::new(pick(&page1, None)?, RouteKind::View)
                    });
                }
            }
        }
    }
    Ok(())
}

/// Conjunction of every `where` along the `from` chain (incl. path globs).
fn declared_filter(name: &str, q: &Query, schema: &filter::Schema) -> Result<filter::Filter> {
    Ok(match q.predicate() {
        // Type-check here: Config cannot see positional schema (§4a, MERGE.md C6a).
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

/// Folds with no `from` (IO.md §4): sitemap, search index; after other routes exist.
pub(crate) fn build_pool_folds(cfg: &Config, db: &mut SiteDb) -> Result<()> {
    let route_schema = crate::schema::site_fields(&cfg.schema.decls, "grackle.toml [schema]")?;
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
            fields: view_fields(v, &route_schema)?,
            ..Route::new(tmpl.to_string(), RouteKind::View)
        });
    }
    Ok(())
}

/// Resolve no-`from` fold members over the final route list (IO.md §4).
pub(crate) fn resolve_pool_folds(cfg: &Config, db: &mut SiteDb, schemas: &Schemas) -> Result<()> {
    for (name, v) in &cfg.views {
        if !v.reads_all_outputs() {
            continue;
        }
        let pred = match &v.filter {
            Some(src) => filter::Filter::parse(src, &route_schema(&schemas.declared_schema()))
                .with_context(|| match &v.filter_profile {
                    Some(p) => {
                        format!("view {name}: filter {src:?} (profile {p} replaced its `where`)")
                    }
                    None => format!("view {name}: filter {src:?}"),
                })?,
            None => filter::Filter::always(),
        };
        let members = db.routes.select(&pred);
        // q53: all-outputs folds see the canonical axis member only.
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
mod object_view_tests {
    use super::*;

    fn cfg(views: &str) -> Config {
        let src = format!(
            "root = \".\"\nextends = \"none\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
             [[collections]]\nname = \"objects\"\n{views}"
        );
        Config::from_toml(&src).expect("test config parses")
    }

    /// Object views scope with `glob(path, ...)` (IO.md §3).
    #[test]
    fn an_object_view_scopes_itself_with_a_path_glob() {
        let c = cfg("[routes.g]\nfrom = \"objects\"\n\
             where = 'glob(path, \"photos/**\")'\n\
             order_by = \"name\"\npath = \"/p/\"\nlayout = \"card\"\n");
        build_views(&c, &mut SiteDb::default(), &Schemas::new(row_schema()))
            .expect("a path glob is object vocabulary");
    }
}

#[cfg(test)]
mod posts_order_tests {
    use super::*;
    use grackle_model::Row;

    fn post(url: &str, date: &str, order: Option<i64>) -> Row {
        let mut r = Row {
            collection: "notes".into(),
            url: url.into(),
            order,
            slug: url.trim_matches('/').into(),
            rel: std::path::PathBuf::from(format!("{}.md", url.trim_matches('/'))),
            rendered: true,
            ..Row::default()
        };
        // Pairing axis: fixture needs the canonical value to be visible.
        r.fields
            .insert("locale".into(), grackle_db::Value::Str("en".into()));
        r.fields
            .insert("date".into(), grackle_db::Value::Str(date.into()));
        r
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
             file = [\"{{date.year}}-{{date.month}}-{{date.day}}-{{slug}}\"]\n\
             [routes.g]\nfrom = \"notes\"\npath = \"/g/\"\nlayout = \"card\"\n{clauses}"
        );
        Config::from_toml(&src).expect("test config parses")
    }

    /// A view with no `order_by` orders by PATH — not newest-first. A posts
    /// collection asks for dates.
    ///
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

        // Undeclared fields remain a load error.
        let e = build_views(&cfg(clauses), &mut db(), &Schemas::new(row_schema())).unwrap_err();
        // `{:#}` for the whole chain (top frame is only "view s: filter ...").
        assert!(format!("{e:#}").contains("unknown field"), "{e:#}");
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
    use grackle_model::Row;

    fn post(date: Option<&str>, tags: &[&str]) -> Row {
        let mut r = Row::default();
        if let Some(d) = date {
            r.fields
                .insert("date".into(), grackle_db::Value::Str(d.into()));
        }
        if !tags.is_empty() {
            r.fields.insert(
                "tags".into(),
                grackle_db::Value::str_list(tags.iter().map(|t| t.to_string())),
            );
        }
        r
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
        let key: Vec<String> = combos[0].iter().map(|k| k.sort.display()).collect();
        assert_eq!(key.join("-"), "2022-03");
    }

    #[test]
    fn undated_rows_are_absent_from_date_partitions() {
        let p = post(None, &["rust"]);
        assert!(key_combos(&p, &["date.year".into()]).is_empty());
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
        // Months must sort numerically (params carry unpadded "3").
        let month = |d| key_combos(&post(Some(d), &[]), &["date.month".to_string()]);
        let (march, december) = (month("2022-03-16"), month("2022-12-16"));
        assert!(
            march[0][0].sort < december[0][0].sort,
            "March must sort before December"
        );
        assert_eq!(march[0][0].sort.display(), "03");
        assert_eq!(
            key_combos(&post(Some("2022-03-16"), &[]), &["date.year".to_string()])[0][0]
                .sort
                .display(),
            "2022"
        );
    }

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
            order: None,
            theme: None,
            shell: None,
            fields: Default::default(),
            images: Default::default(),
            logical: "recipes/carbonara.md".into(),
            claimed: false,
            ..Default::default()
        };
        p.fields
            .insert("locale".into(), grackle_db::Value::Str("en".into()));
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

        p.fields.clear();
        assert!(key_combos(&p, &["course".into()]).is_empty());

        // Undated page absent from year partition, same as undated post.
        assert!(key_combos(&p, &["date.year".into()]).is_empty());
        p.fields.insert(
            "date".into(),
            grackle_db::filter::Value::Str("2026-07-01".into()),
        );
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
    fn month_param_is_unpadded() {
        let p = post(Some("2022-12-16"), &[]);
        let keys = group_keys(&p, "date.month");
        assert!(keys[0].params.contains(&("month".into(), "12".into())));
        assert!(keys[0].params.iter().all(|(k, _)| k != "month_name"));
    }
}

/// Sequence per collection (q51), not a post-filter.
#[cfg(test)]
mod adjacency_tests {
    use super::*;
    use grackle_model::Row;

    fn post(collection: &str, url: &str, date: Option<&str>, draft: bool) -> Row {
        let mut r = Row {
            rel: std::path::PathBuf::from(format!(
                "{collection}/{}.md",
                url.trim_matches('/').replace('/', "-")
            )),
            collection: collection.into(),
            url: url.into(),
            slug: url.trim_matches('/').replace('/', "-"),
            fields: BTreeMap::from([("draft".to_string(), grackle_db::Value::Bool(draft))]),
            ..Row::default()
        };
        if let Some(d) = date {
            r.fields
                .insert("date".into(), grackle_db::Value::Str(d.into()));
        }
        r.fields
            .insert("locale".into(), grackle_db::Value::Str("en".into()));
        r
    }

    fn db_with(rows: Vec<Row>) -> SiteDb {
        SiteDb::seed(rows, true)
    }

    fn cfg(extra: &str) -> Config {
        Config::from_toml(&format!(
            "root = \".\"\nextends = \"none\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
             [schema]\ndate = {{ type = \"date\" }}\ndraft = {{ type = \"bool\" }}\n\
             [[collections]]\nname = \"posts\"\nsource = \"_posts\"\n\
             file = [\"{{date.year}}-{{date.month}}-{{date.day}}-{{slug}}\"]\n{extra}"
        ))
        .expect("test config parses")
    }

    fn schemas(c: &Config) -> Schemas {
        let mut s = Schemas::new(row_schema());
        s.set_site(c.schema.decls.clone(), "[schema]")
            .expect("test schema");
        s
    }

    fn seq(db: &SiteDb, collection: &str) -> Vec<String> {
        db.adjacency[collection]
            .iter()
            .filter_map(|k| db.rows.get(k))
            .map(|r| r.url.clone())
            .collect()
    }

    /// One sequence per collection: neighbours stay within the corpus.
    #[test]
    fn each_collection_gets_its_own_sequence() {
        let c = cfg("[[collections]]\nname = \"notes\"\n\
                     source = \"_notes\"\nfile = [\"{{date.year}}-{{date.month}}-{{date.day}}-{{slug}}\"]\n");
        let mut db = db_with(vec![
            post("posts", "/blog/jan/", Some("2026-01-01"), false),
            post("notes", "/notes/feb/", Some("2026-02-01"), false),
            post("posts", "/blog/mar/", Some("2026-03-01"), false),
            post("notes", "/notes/apr/", Some("2026-04-01"), false),
        ]);
        build_adjacency(&c, &mut db, &schemas(&c)).unwrap();
        assert_eq!(seq(&db, "posts"), ["/blog/mar/", "/blog/jan/"]);
        assert_eq!(seq(&db, "notes"), ["/notes/apr/", "/notes/feb/"]);
    }

    /// Diagnostic walk includes drafts; rendered neighbours are §6g relations.
    #[test]
    fn the_diagnostic_walk_is_chronological_and_unfiltered() {
        let c = cfg("");
        let mut db = db_with(vec![
            post("posts", "/blog/jan/", Some("2026-01-01"), false),
            post("posts", "/blog/feb/", Some("2026-02-01"), true), // dated DRAFT
            post("posts", "/blog/mar/", Some("2026-03-01"), false),
        ]);
        build_adjacency(&c, &mut db, &schemas(&c)).unwrap();
        assert_eq!(
            seq(&db, "posts"),
            ["/blog/mar/", "/blog/feb/", "/blog/jan/"],
            "the raw table walk includes a dated draft — the relation engine, \
             not this sequence, is what excludes it from a page"
        );
    }
}

//! The dev-server inspector's payload (`/__debug/site.json`).
//!
//! A purpose-built serialization, deliberately *not* `grackle export`. The
//! export is the database as the database sees it; this is the database as a
//! person diagnosing it needs to see it, which means two differences:
//!
//! 1. It carries what `export` skips. Route `members` and the row flags are
//!    `#[serde(skip)]` there, and they are exactly what answers "what picks
//!    this up" and "why is this missing".
//! 2. Members are emitted as **URLs, not indices**. An index is only
//!    meaningful next to the table it indexes; a URL joins to everything the
//!    inspector already has, so the client needs no lookup tables and no
//!    knowledge of which table a view ranges over.
//!
//! Serve-only: nothing here is emitted into a build.

use anyhow::Result;
use serde::Serialize;

use crate::config::Config;
use crate::model::SiteDb;

#[derive(Serialize)]
struct Payload<'a> {
    site: Site<'a>,
    stats: Stats,
    posts: Vec<Row>,
    pages: Vec<Row>,
    objects: Vec<Row>,
    routes: Vec<Route>,
    views: Vec<View>,
}

#[derive(Serialize)]
struct Site<'a> {
    title: &'a str,
    url: &'a str,
    locales: Vec<&'a str>,
    default_locale: &'a str,
}

#[derive(Serialize)]
struct Stats {
    posts: usize,
    pages: usize,
    objects: usize,
    routes: usize,
    load_ms: f64,
}

/// One row of any table, flattened to what the inspector shows. `table`
/// distinguishes them; absent fields are simply omitted per table.
#[derive(Serialize)]
struct Row {
    table: &'static str,
    url: String,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    layout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shell: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    theme: Option<String>,
    /// A tree row with no front matter is copied, not rendered.
    rendered: bool,
    /// A claimed row has no route of its own, its landing owns the URL.
    claimed: bool,
    /// Typed schema fields, stringified for display.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    fields: Vec<(String, String)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u64>,
}

#[derive(Serialize)]
struct Route {
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    view: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    page: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    params: Vec<(String, String)>,
    /// Member row URLs, in the order the view materialized them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    members: Vec<String>,
}

#[derive(Serialize)]
struct View {
    name: String,
    /// The view's `from`, the pool it ranges over. Named for the config
    /// key, which is what a reader of the inspector has in front of them.
    #[serde(skip_serializing_if = "Option::is_none")]
    from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    base: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    layout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shell: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    group_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    paginate: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    routes: Vec<String>,
    /// How many routes this view materialized, the fan-out that makes 7
    /// views responsible for most of the URL space.
    route_count: usize,
}

pub fn payload(cfg: &Config, db: &SiteDb) -> Result<Vec<u8>> {
    // Post `rel` is collection-relative; pages are root-relative. The tree
    // lens joins them into one filesystem, so re-root the posts, from the
    // row's own absolute path, not from a collection's source, because
    // several collections feed this table and only the path knows which one
    // a row came from.
    let rel_to_root = |abs: &std::path::Path| {
        abs.strip_prefix(cfg.root())
            .unwrap_or(abs)
            .to_string_lossy()
            .to_string()
    };
    let posts: Vec<Row> = db
        .rows
        .iter()
        .filter(|p| cfg.body_held(p))
        .map(|p| Row {
            table: "posts",
            url: p.url.clone(),
            path: rel_to_root(&p.path),
            title: p.title.clone(),
            date: p.as_date("date").map(|d| d.to_string()),
            layout: None,
            shell: None,
            theme: None,
            rendered: true,
            claimed: false,
            fields: p
                .fields
                .iter()
                .map(|(k, v)| (k.clone(), value_text(v)))
                .collect(),
            size: None,
        })
        .collect();

    let pages: Vec<Row> = db
        .rows
        .iter()
        .filter(|p| {
            cfg.collections
                .get(&p.collection)
                .is_some_and(|c| c.is_tree())
        })
        .map(|p| Row {
            table: "pages",
            url: p.url.clone(),
            path: p.rel.to_string_lossy().to_string(),
            title: p.title.clone(),
            date: None,
            layout: None,
            shell: p.shell.clone(),
            theme: p.theme.clone(),
            rendered: p.rendered,
            claimed: p.claimed,
            fields: p
                .fields
                .iter()
                .map(|(k, v)| (k.clone(), value_text(v)))
                .collect(),
            size: Some(p.size),
        })
        .collect();

    let objects: Vec<Row> = db
        .by_name
        .values()
        .flatten()
        .filter_map(|k| db.rows.get(k))
        .map(|o| Row {
            table: "objects",
            url: o.url.clone(),
            path: o.rel.to_string_lossy().to_string(),
            title: None,
            date: None,
            layout: None,
            shell: None,
            theme: None,
            rendered: false,
            claimed: false,
            fields: Vec::new(),
            size: Some(o.size),
        })
        .collect();

    // A fold with no `from` ranges over ROUTES, not a table, so it
    // carries no `members`, the render passes re-evaluate its filter. The
    // set is real all the same, so evaluate it here rather than show an
    // empty list and imply otherwise.
    let pool_members = |name: &str| -> Vec<String> {
        let Some(v) = cfg.views.get(name) else {
            return Vec::new();
        };
        let pred = match &v.filter {
            Some(src) => {
                match crate::filter::Filter::parse(src, &crate::model::route_schema(&db.declared)) {
                    Ok(p) => p,
                    Err(_) => return Vec::new(),
                }
            }
            None => crate::filter::Filter::always(),
        };
        db.routes.matching(&pred).map(|r| r.url.clone()).collect()
    };

    // Members are keys into the one row store, so the base kind does
    // not enter into resolving them.
    let member_urls = |r: &crate::model::Route| -> Vec<String> {
        let Some(view) = r.view.as_deref() else {
            return Vec::new();
        };
        if cfg.views.get(view).is_some_and(|v| v.reads_all_outputs()) {
            return pool_members(view);
        }
        r.members
            .iter()
            .filter_map(|k| db.rows.get(k).map(|p| p.url.clone()))
            .collect()
    };

    let routes: Vec<Route> = db
        .routes
        .iter()
        .map(|r| Route {
            url: r.url.clone(),
            source: r.source.as_ref().map(|p| {
                p.strip_prefix(&cfg.root)
                    .unwrap_or(p)
                    .to_string_lossy()
                    .to_string()
            }),
            view: r.view.clone(),
            key: r.key.clone(),
            page: r.page,
            params: r.params.clone(),
            members: member_urls(r),
        })
        .collect();

    let views: Vec<View> = cfg
        .views
        .iter()
        .map(|(name, v)| {
            let mine: Vec<String> = db
                .routes
                .iter()
                .filter(|r| r.view.as_deref() == Some(name.as_str()))
                .map(|r| r.url.clone())
                .collect();
            View {
                name: name.clone(),
                from: v.from.as_ref().map(|f| f.display()),
                base: cfg.query(name).ok().map(|q| q.base.join(", ")),
                layout: v.layout.clone(),
                shell: v.shell.clone(),
                filter: v.filter.clone(),
                group_by: v.group_by.clone(),
                paginate: v.paginate,
                route_count: mine.len(),
                routes: mine.into_iter().take(200).collect(),
            }
        })
        .collect();

    let p = Payload {
        site: Site {
            title: cfg.site_title(cfg.pairing_canonical().unwrap_or("")),
            url: &cfg.site.url,
            locales: match cfg.pairing_axis() {
                Some((_, a)) => a.values.iter().map(String::as_str).collect(),
                None => Vec::new(),
            },
            default_locale: cfg.pairing_canonical().unwrap_or(""),
        },
        stats: Stats {
            posts: db.rows.iter().filter(|r| cfg.body_held(r)).count(),
            pages: db
                .rows
                .iter()
                .filter(|r| {
                    cfg.collections
                        .get(&r.collection)
                        .is_some_and(|c| c.is_tree())
                })
                .count(),
            objects: db.by_name.values().map(|v| v.len()).sum(),
            routes: db.routes.len(),
            load_ms: db.stats.read_ms + db.stats.index_ms + db.stats.views_ms,
        },
        posts,
        pages,
        objects,
        routes,
        views,
    };
    Ok(serde_json::to_vec(&p)?)
}

/// `/__debug/` is a closed namespace: anything under it belongs to the
/// inspector, so a site page at that prefix cannot shadow it.
pub fn is_debug_path(path: &str) -> bool {
    path == "/__debug" || path.starts_with("/__debug/")
}

/// The inspector's own assets, embedded in the binary (serve-only, a build
/// never emits these).
pub fn asset(path: &str) -> Option<(&'static str, &'static [u8])> {
    match path {
        "/__debug" | "/__debug/" => Some((
            "text/html; charset=utf-8",
            include_bytes!("../assets/debug.html"),
        )),
        "/__debug/debug.css" => Some((
            "text/css; charset=utf-8",
            include_bytes!("../assets/debug.css"),
        )),
        "/__debug/debug.js" => Some((
            "text/javascript; charset=utf-8",
            include_bytes!("../assets/debug.js"),
        )),
        _ => None,
    }
}

/// A typed field as one display string. The inspector shows values, not
/// types, an int and a string that both read `4` are the same to the eye,
/// and the schema is one click away when the difference matters.
pub fn value_text(v: &crate::filter::Value) -> String {
    use crate::filter::Value as V;
    match v {
        V::Str(s) => s.clone(),
        V::Int(i) => i.to_string(),
        V::Bool(b) => b.to_string(),
        V::List(items) => items
            .iter()
            .filter_map(|x| match x {
                V::Str(s) => Some(s.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(", "),
        V::Null => String::new(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// Filterable facts `grackle explain` prints for a row: `collection`,
/// `shell`, `rule`, `rendered`, `front_mattered` (with provenance),
/// `output`, and `strong_url` when present.
///
/// `collection` + `rule` answer which scope and glob claimed the file.
/// `rendered` is the stored `shell::renders` bit (sidecars can disagree with
/// a re-derivation from the block alone). `output` is join: where the row
/// lands, or why it does not (`claimed` / `on demand` / embed-addressed).
pub fn row_facts(r: &crate::model::Row) -> String {
    let identity = match (r.front_mattered, r.sidecar) {
        (false, _) => "false".to_string(),
        (true, false) => "true (block)".to_string(),
        (true, true) => "true (sidecar)".to_string(),
    };
    let output = match (&r.output, r.claimed, r.on_demand, &r.strong_url) {
        (Some(k), _, _, _) => k.to_string(),
        (None, true, _, _) => "- (claimed — the landing at that URL owns it)".to_string(),
        (None, _, true, _) => "- (on demand — nothing has referenced it)".to_string(),
        (None, _, _, Some(_)) => "- (embed-addressed — nothing has embedded it)".to_string(),
        (None, _, _, _) => "-".to_string(),
    };
    let strong = match &r.strong_url {
        Some(s) => format!("strong_url  {s}\n"),
        None => String::new(),
    };
    format!(
        "collection  {}\nrule        {}\nshell       {}\nfront_mattered {identity}\nrendered    {}\noutput      {output}\n{strong}",
        r.collection,
        r.rule.as_deref().unwrap_or("-"),
        r.shell.as_deref().unwrap_or("-"),
        r.rendered,
    )
}

/// The join's two list fields, as `explain` prints them.
///
/// A capped list rather than a count: on grack.com a post is carried by five
/// or six listings and a fold's `inputs` is the whole site, so the useful
/// shape is "the first few, and how many there are". Sorted at construction,
/// so what a reader sees is stable across runs.
pub fn join_list(name: &str, keys: &[crate::model::Key]) -> String {
    let lines: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
    capped_list(name, &lines)
}

/// `join_list`'s shape over already-rendered lines, what `pull` prints its
/// edge list and its work order with. One formatter, because two
/// surfaces that cap differently are two surfaces a reader has to learn.
pub fn capped_list(name: &str, lines: &[String]) -> String {
    const SHOWN: usize = 8;
    if lines.is_empty() {
        return format!("{name:<11} -\n");
    }
    let mut out = format!("{name:<11} {}\n", lines.len());
    for l in lines.iter().take(SHOWN) {
        out.push_str(&format!("            {l}\n"));
    }
    if lines.len() > SHOWN {
        out.push_str(&format!("            … and {} more\n", lines.len() - SHOWN));
    }
    out
}

/// The rest of `explain`'s row block: the cascade keys the engine reads off
/// the row by name, then every other declared field.
///
/// **A cascade key is two things at once**, and that is the whole bug this
/// closes. The base declares `theme`/`shell`/`slot` in its
/// base `[schema]` so a marker's or a rule's value for one of them travels
/// the same typed cascade as any other field, which makes each of them a
/// named field on `Row` *and* a column in `Row.fields`. A surface that prints
/// the named field and then dumps the columns prints it twice: `explain` did,
/// for every row that resolved a `layout`.
///
/// **The named line wins and the dump skips the name**, following the facts pass's
/// resolution for `shell`, because only the named line can answer for a row
/// that resolved nothing, the dump prints a field only where a value landed,
/// so its silence is indistinguishable from "no such key". Hence `-` for an
/// unresolved `theme` or `slot`.
///
/// `shell` is printed one block up, by `row_facts`, beside `front_mattered`
/// and the output URL, not because it belongs to a different family. The
/// skip below covers the cascade list.
pub fn row_fields(r: &crate::model::Row) -> String {
    let slot = match r.fields.get("slot") {
        Some(grackle_db::Value::Str(s)) => s.as_str(),
        _ => "-",
    };
    let mut out = format!(
        "slot        {}\ntheme       {}\n",
        slot,
        r.theme.as_deref().unwrap_or("-"),
    );
    for (name, value) in &r.fields {
        if grackle_source::schema::CASCADE
            .iter()
            .any(|(n, _)| n == name)
        {
            continue;
        }
        out.push_str(&format!("{name:<11} {}\n", value_text(value)));
    }
    out
}

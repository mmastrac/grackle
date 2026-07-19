//! The dev-server inspector's payload (`/__debug/site.json`).
//!
//! A purpose-built serialization, deliberately *not* `grackle export`. The
//! export is the database as the database sees it; this is the database as a
//! person diagnosing it needs to see it — which means two differences:
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

use crate::config::{Config, Kind};
use crate::db::SiteDb;

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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    layout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shell: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    theme: Option<String>,
    locale: String,
    /// Diagnosis flags: why a row might not appear where expected.
    draft: bool,
    hidden: bool,
    noindex: bool,
    /// A tree row with no front matter is copied, not rendered.
    rendered: bool,
    /// q45: a claimed row has no route of its own — its landing owns the URL.
    claimed: bool,
    /// Typed schema fields (§5b), stringified for display.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    fields: Vec<(String, String)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u64>,
}

#[derive(Serialize)]
struct Route {
    url: String,
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    view: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    page: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    locale: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    params: Vec<(String, String)>,
    /// Member row URLs, in the order the view materialized them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    members: Vec<String>,
}

#[derive(Serialize)]
struct View {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    over: Option<String>,
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
    /// How many routes this view materialized — the fan-out that makes 7
    /// views responsible for most of the URL space.
    route_count: usize,
}

pub fn payload(cfg: &Config, db: &SiteDb) -> Result<Vec<u8>> {
    // Post `rel` is collection-relative; pages are root-relative. The tree
    // lens joins them into one filesystem, so re-root the posts here rather
    // than teaching the client about collection sources.
    let posts_dir = cfg
        .collections
        .values()
        .find(|c| c.kind == Kind::Posts)
        .and_then(|c| c.source.clone())
        .unwrap_or_default();
    let posts: Vec<Row> = db
        .posts
        .rows
        .iter()
        .map(|p| Row {
            table: "posts",
            url: p.url.clone(),
            path: if posts_dir.is_empty() {
                p.rel.to_string_lossy().to_string()
            } else {
                format!("{posts_dir}/{}", p.rel.to_string_lossy())
            },
            title: Some(p.title.clone()),
            date: p.date.map(|d| d.to_string()),
            tags: p.tags.clone(),
            layout: p.layout.clone(),
            shell: None,
            theme: None,
            locale: p.locale.clone(),
            draft: p.draft,
            hidden: p.hidden,
            noindex: p.noindex,
            rendered: true,
            claimed: false,
            fields: Vec::new(),
            size: None,
        })
        .collect();

    let pages: Vec<Row> = db
        .pages
        .rows
        .iter()
        .map(|p| Row {
            table: "pages",
            url: p.url.clone(),
            path: p.rel.to_string_lossy().to_string(),
            title: p.title.clone(),
            date: None,
            tags: Vec::new(),
            layout: p.layout.clone(),
            shell: p.shell.clone(),
            theme: p.theme.clone(),
            locale: p.locale.clone(),
            draft: false,
            hidden: p.hidden,
            noindex: p.noindex,
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
        .objects
        .rows
        .iter()
        .map(|o| Row {
            table: "objects",
            url: o.url.clone(),
            path: o.rel.to_string_lossy().to_string(),
            title: None,
            date: None,
            tags: Vec::new(),
            layout: None,
            shell: None,
            theme: None,
            locale: String::new(),
            draft: false,
            hidden: false,
            noindex: false,
            rendered: false,
            claimed: false,
            fields: Vec::new(),
            size: Some(o.size),
        })
        .collect();

    // A star view (`over = "*"`) ranges over ROUTES, not a table, so it
    // carries no `members` — the render passes re-evaluate its filter. The
    // set is real all the same (the search index has 327 documents), so
    // evaluate it here rather than show an empty list and imply otherwise.
    let star_members = |name: &str| -> Vec<String> {
        let Some(v) = cfg.views.get(name) else { return Vec::new() };
        let pred = match &v.filter {
            Some(src) => match crate::filter::Filter::parse(src, &crate::db::route_schema()) {
                Ok(p) => p,
                Err(_) => return Vec::new(),
            },
            None => crate::filter::Filter::always(),
        };
        db.routes
            .iter()
            .filter(|r| pred.eval(*r))
            .map(|r| r.url.clone())
            .collect()
    };

    // Members are indices into the view's base table, so resolving them to
    // URLs needs the base kind — the one thing a client could not work out
    // for itself.
    let member_urls = |r: &crate::db::Route| -> Vec<String> {
        let Some(view) = r.view.as_deref() else { return Vec::new() };
        if cfg.views.get(view).is_some_and(|v| v.over == "*") {
            return star_members(view);
        }
        let Some(kind) = cfg
            .query(view)
            .ok()
            .and_then(|q| cfg.collections.get(&q.base).map(|c| c.kind))
        else {
            return Vec::new();
        };
        r.members
            .iter()
            .filter_map(|&i| match kind {
                Kind::Posts => db.posts.rows.get(i).map(|p| p.url.clone()),
                Kind::Tree => db.pages.rows.get(i).map(|p| p.url.clone()),
                Kind::Objects => db.objects.rows.get(i).map(|o| o.url.clone()),
            })
            .collect()
    };

    let routes: Vec<Route> = db
        .routes
        .iter()
        .map(|r| Route {
            url: r.url.clone(),
            kind: r.kind.as_str(),
            source: r.source.as_ref().map(|p| {
                p.strip_prefix(&cfg.root)
                    .unwrap_or(p)
                    .to_string_lossy()
                    .to_string()
            }),
            view: r.view.clone(),
            key: r.key.clone(),
            page: r.page,
            locale: r.locale.clone(),
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
                over: Some(v.over.clone()).filter(|s| !s.is_empty()),
                base: cfg.query(name).ok().map(|q| q.base),
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
            title: &cfg.site.title,
            url: &cfg.site.url,
            locales: cfg.i18n.locales.iter().map(String::as_str).collect(),
            default_locale: &cfg.i18n.default,
        },
        stats: Stats {
            posts: db.posts.rows.len(),
            pages: db.pages.rows.len(),
            objects: db.objects.rows.len(),
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

/// The inspector's own assets, embedded in the binary (serve-only — a build
/// never emits these).
pub fn asset(path: &str) -> Option<(&'static str, &'static [u8])> {
    match path {
        "/__debug" | "/__debug/" => {
            Some(("text/html; charset=utf-8", include_bytes!("../assets/debug.html")))
        }
        "/__debug/debug.css" => {
            Some(("text/css; charset=utf-8", include_bytes!("../assets/debug.css")))
        }
        "/__debug/debug.js" => Some((
            "text/javascript; charset=utf-8",
            include_bytes!("../assets/debug.js"),
        )),
        _ => None,
    }
}

/// A typed field as one display string. The inspector shows values, not
/// types — an int and a string that both read `4` are the same to the eye,
/// and the schema is one click away when the difference matters.
fn value_text(v: &crate::filter::Value) -> String {
    use crate::filter::Value as V;
    match v {
        V::Str(s) => s.clone(),
        V::Int(i) => i.to_string(),
        V::Bool(b) => b.to_string(),
        V::Null => String::new(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

//! Shared preview / theme / URL helpers used by listing passes and emit.
//!
//! Lives under `passes` so the listing pass does not reach into the pipeline
//! for its helpers (the old upward dependency).

use anyhow::Result;
use std::path::Path;

use crate::config::{Collection, Config, View};
use crate::model::{Route, SiteDb};
use crate::parts;
use crate::pipeline::types::PageBody;
use crate::theme;

/// The site-global facts the chrome parts fill from: which capability
/// routes materialized. Computed once per build.
pub struct ChromeFacts {
    /// A `shell = "search"` route exists.
    search: bool,
    /// The first `shell = "atom"` route's URL, in route order.
    feed_url: Option<String>,
}

impl ChromeFacts {
    pub(crate) fn of(cfg: &Config, db: &SiteDb) -> ChromeFacts {
        let mut search = false;
        let mut feed_url = None;
        for r in &db.routes {
            let Some(v) = r.view.as_ref().and_then(|v| cfg.views.get(v)) else {
                continue;
            };
            match v.shell.as_deref() {
                Some("search") => search = true,
                Some("atom") => {
                    if let Some(first) = &feed_url {
                        eprintln!(
                            "grackle: two atom routes ({first} and {}) — the feed \
                             chrome part links the first",
                            r.url
                        );
                    } else {
                        feed_url = Some(r.url.clone());
                    }
                }
                _ => {}
            }
        }
        ChromeFacts { search, feed_url }
    }
}

/// The chrome-part inputs for one page: the facts, with labels resolved at
/// the page's locale. The scheme fill decision stays with the theme
/// (`Theme::scheme_choice_offered`); only its labels travel here.
pub(crate) fn chrome_input(cfg: &Config, facts: &ChromeFacts, lang: &str) -> theme::ChromeInput {
    let s = |key: &str| cfg.i18n_string(key, lang).to_string();
    theme::ChromeInput {
        search: facts
            .search
            .then(|| (s("search"), format!("{}/search.js", cfg.site.baseurl))),
        feed: facts
            .feed_url
            .as_ref()
            .map(|u| (s("feed"), format!("{}{u}", cfg.site.baseurl))),
        scheme: theme::SchemeLabels {
            auto: s("scheme_auto"),
            light: s("scheme_light"),
            dark: s("scheme_dark"),
        },
        skip: s("skip"),
    }
}

/// One member of an axis pool for a route: the row/route expressions read,
/// its published URL, the member code, and whether it is the current page.
pub(crate) struct AxisPoolMember<'a> {
    pub row: &'a dyn crate::filter::Row,
    pub url: &'a str,
    pub member: String,
    pub current: bool,
}

/// Include-self siblings of this route on a declared axis.
///
/// A **file axis** pivots through translation files (`by_logical`); a **route
/// axis** (or a view) through sibling routes that differ in exactly that axis.
/// Self is included, `hreflang` asks every version to list every version, and
/// the axis switcher flags the current one.
pub(crate) fn axis_pool<'a>(
    cfg: &Config,
    db: &'a SiteDb,
    r: &'a Route,
    axis_name: &str,
) -> Vec<AxisPoolMember<'a>> {
    let Some(axis) = cfg.axes.get(axis_name) else {
        return Vec::new();
    };
    let field = axis.field.as_str();
    let member_of = |row: &dyn crate::filter::Row| -> String {
        match row.field(field) {
            crate::filter::Value::Str(s) if !s.is_empty() => s,
            _ => axis.canonical().unwrap_or("").to_owned(),
        }
    };

    if cfg.is_file_axis(axis_name) {
        if let Some(k) = &r.row {
            return db
                .rows
                .get(k)
                .and_then(|p| db.by_logical.get(&p.logical))
                .into_iter()
                .flatten()
                .filter_map(|sk| db.rows.get(sk))
                .filter(|s| !s.url.is_empty())
                .map(|s| AxisPoolMember {
                    row: s,
                    url: s.url.as_str(),
                    member: member_of(s),
                    current: Some(&s.key) == r.row.as_ref(),
                })
                .collect();
        }
        // View on a file axis: hold route-axis members fixed, vary this field.
        let in_scope = |o: &Route| -> bool {
            o.view.is_some() && o.view == r.view && o.key == r.key && o.page == r.page
        };
        return db
            .routes
            .iter()
            .filter(|o| in_scope(o) && o.axis == r.axis)
            .map(|o| AxisPoolMember {
                row: o,
                url: o.url.as_str(),
                member: member_of(o),
                current: o.url == r.url,
            })
            .collect();
    }

    // Route axis: pivot one member, hold the rest (and pairing axis) fixed.
    let in_scope = |o: &Route| -> bool {
        match &r.row {
            Some(k) => o.row.as_ref() == Some(k),
            None => o.view.is_some() && o.view == r.view && o.key == r.key && o.page == r.page,
        }
    };
    let cur_pairing = cfg.pairing_member(r);
    axis.values
        .iter()
        .filter_map(|v| {
            db.routes
                .iter()
                .find(|o| {
                    in_scope(o)
                        && cfg.pairing_member(*o) == cur_pairing
                        && o.axis.len() == r.axis.len()
                        && r.axis.iter().all(|rm| {
                            let want = if rm.axis == axis_name {
                                v.as_str()
                            } else {
                                rm.value.as_str()
                            };
                            o.axis
                                .iter()
                                .any(|om| om.axis == rm.axis && om.value == want)
                        })
                })
                .map(|o| AxisPoolMember {
                    row: o,
                    url: o.url.as_str(),
                    member: v.clone(),
                    current: o.url == r.url,
                })
        })
        .collect()
}

/// The axis slot: every axis THIS route is a member of, each a group
/// of member links with the current one flagged, the switcher a theme renders,
/// for a row page or a listing view alike.
///
/// File-axis members come from [`axis_pool`] / `by_logical`; route-axis members
/// from sibling routes. A group with fewer than two members drops out.
pub(crate) fn axes_part(cfg: &Config, db: &SiteDb, r: &Route) -> Vec<parts::PartMap> {
    let mut groups = Vec::new();
    let cur_pairing = cfg.pairing_member(r);
    let mut seen = std::collections::BTreeSet::new();

    // Pairing / file axis first (by_logical pool), then each route-axis member.
    if let Some((name, _)) = cfg.pairing_axis() {
        if cfg.is_file_axis(name) {
            seen.insert(name.to_string());
            let members: Vec<(String, String, bool)> = axis_pool(cfg, db, r, name)
                .into_iter()
                .map(|m| {
                    (
                        cfg.i18n.name_of(&m.member).to_string(),
                        m.url.to_string(),
                        m.current,
                    )
                })
                .collect();
            if let Some(g) =
                parts::axis_group(name, cfg.i18n_string("translations", &cur_pairing), members)
            {
                groups.push(g);
            }
        }
    }

    for m in &r.axis {
        if !seen.insert(m.axis.clone()) {
            continue;
        }
        let members: Vec<(String, String, bool)> = axis_pool(cfg, db, r, &m.axis)
            .into_iter()
            .map(|m| (m.member, m.url.to_string(), m.current))
            .collect();
        if let Some(g) = parts::axis_group(&m.axis, &m.axis, members) {
            groups.push(g);
        }
    }
    groups
}

/// The collection at the base of a view's `from` chain, whose role (read off
/// its `source`, now that `kind` is gone) decides which render pass owns the
/// view's routes. None for a fold over every output, which has no collection
/// under it.
pub(crate) fn view_base_collection<'a>(cfg: &'a Config, view: &str) -> Option<&'a Collection> {
    // A union's members share a role, they share a `from` vocabulary, so the
    // first answers for the whole base.
    let base = cfg.query(view).ok()?.base;
    cfg.collections.get(base.first()?)
}

/// The link resolver a page hands its slot fills: the fill's owner
/// directory is the relative base, and the consuming page's locale drives
/// view links, one nav.md serves every locale. The impossible `url_dir`
/// disables the browser-agreement bypass: fills are shared across pages,
/// so the canonical URL is the only correct answer.
pub(crate) fn fill_link_resolver<'a>(
    cfg: &'a Config,
    space: &'a crate::links::LinkSpace,
    locale: &'a str,
) -> impl Fn(crate::links::Cite, &Path, &str) -> Result<Option<String>> + 'a {
    move |form: crate::links::Cite, owner: &Path, href: &str| {
        crate::links::resolve(
            cfg,
            space,
            owner,
            "\u{0}",
            locale,
            &format!("{}/.slots", owner.display()),
            form,
            href,
        )
    }
}

/// What an axis member sets, when it sets the field asked for.
///
/// A member declares which row field its value stands in for, `theme` renders
/// one corpus several ways, `shell` gives a document its md twin, so a render
/// path asks for the field it cares about and gets `None` on every route that
/// is not a member of an axis about that field. The value beats the row's own:
/// the member IS the alternative form, and a row that named a theme named it
/// for its canonical self.
pub(crate) fn axis_field<'a>(r: &'a Route, field: &str) -> Option<&'a str> {
    r.axis
        .iter()
        .find(|a| a.field == field)
        .map(|a| a.value.as_str())
}

/// Theme for a route: axis member beats `next` (row or view theme).
pub(crate) fn resolve_theme<'a>(
    themes: &'a theme::Themes,
    r: &'a Route,
    next: Option<&'a str>,
) -> (Option<&'a str>, Option<String>) {
    themes.resolve(axis_field(r, "theme").or(next))
}

/// Listing/landing: axis or view theme, else `fallback`.
pub(crate) fn resolve_view_theme<'a>(
    themes: &'a theme::Themes,
    r: &'a Route,
    view_theme: Option<&'a str>,
    fallback: impl FnOnce() -> (Option<&'a str>, Option<String>),
) -> (Option<&'a str>, Option<String>) {
    match axis_field(r, "theme").or(view_theme) {
        Some(spec) => themes.resolve(Some(spec)),
        None => fallback(),
    }
}

/// Pagination for a paginated route (those carrying a page number); an
/// unpaginated grouped view has `page: None` and gets nothing.
///
/// Page URLs come from the owning view's already-materialized pages rather
/// than re-rendering route templates with `{n}`. A grouped template also
/// carries `{key}`, which a `{n}`-only renderer cannot fill, and `total`
/// must count pages within one group, not across every group of the view.
/// A materialized URL already wears its group key, its record slug (`{key}`
/// is slugged in the URL and not in the params), and its locale prefix.
/// Pages are only created where rows exist, so the sibling list is exactly
/// the pages there are.
pub(crate) fn pagination_parts(
    cfg: &Config,
    db: &SiteDb,
    _view: &str,
    _v: &View,
    r: &Route,
) -> Result<Option<parts::PartMap>> {
    let Some(cur) = r.page else { return Ok(None) };
    // Same view, same i18n member, same GROUP: pagination is per partition, and two
    // routes of one view are in the same partition when their group params
    // agree (empty for an ungrouped view, so it degenerates correctly).
    let mut siblings: Vec<&Route> = db
        .routes
        .iter()
        .filter(|x| {
            x.view == r.view
                && x.page.is_some()
                && match cfg.pairing_axis() {
                    Some((n, _)) => cfg.same_on(*x, r, n),
                    None => true,
                }
                && x.params == r.params
        })
        .collect();
    siblings.sort_by_key(|x| x.page);
    let urls: Vec<String> = siblings.iter().map(|x| x.url.clone()).collect();
    Ok(parts::pagination(cur, &urls))
}

#[allow(clippy::too_many_arguments)]
/// Members of a view/route as row projections, objects, truncated prose, or
/// tree bodies from `page_bodies` when the post map has none.
pub(crate) fn member_rows(
    cfg: &Config,
    schemas: &crate::parts::Schemas,
    resolve_asset: &dyn Fn(&str) -> String,
    db: &crate::model::SiteDb,
    view: &str,
    members: &[grackle_model::Key],
    thumbs: &crate::thumbs::Renditions,
    bodies: &std::collections::HashMap<&grackle_model::Key, crate::markdown::Doc>,
    page_bodies: &std::collections::HashMap<String, PageBody>,
    is_object: impl Fn(&grackle_model::Key) -> bool,
) -> anyhow::Result<Vec<parts::PartMap>> {
    let summary = cfg
        .fields_for(view)
        .get("summary")
        .and_then(|f| f.as_expr())
        .map(|src| {
            grackle_db::FieldExpr::parse(src, &cfg.field_expr_schema(), grackle_db::Type::Content)
                .expect("summary field validated at load")
        });
    let mut out = Vec::new();
    for k in members {
        let Some(p) = db.rows.get(k) else {
            continue;
        };
        let m = if is_object(&p.key) {
            object_row(cfg, schemas, resolve_asset, p, thumbs)?
        } else {
            let (html, truncated) = match bodies.get(&p.key) {
                Some(d) => match &summary {
                    Some(expr) => {
                        let c = match expr.eval(&FieldBind {
                            row: p,
                            content: grackle_db::Content::new(d.blocks.clone()),
                        }) {
                            grackle_db::Value::Content(c) => c,
                            _ => grackle_db::Content::new(d.blocks.clone()),
                        };
                        (c.html_string(), c.truncated)
                    }
                    None => (d.whole.clone(), false),
                },
                None => (
                    page_bodies
                        .get(&p.url)
                        .map(|pb| pb.frag.clone())
                        .unwrap_or_default(),
                    false,
                ),
            };
            content_row(
                cfg,
                schemas,
                resolve_asset,
                view,
                p,
                thumbs,
                Some(html),
                truncated,
                bodies.get(&p.key).map(|d| d.blocks.as_slice()),
            )?
        };
        out.push(m);
    }
    Ok(out)
}

/// Binds `content` plus the underlying row for `fields.NAME` expressions.
pub(crate) struct FieldBind<'a> {
    pub row: &'a crate::model::Row,
    pub content: grackle_db::Content,
}

impl grackle_db::Row for FieldBind<'_> {
    fn field(&self, name: &str) -> grackle_db::Value {
        match name {
            "content" => grackle_db::Value::Content(self.content.clone()),
            other => grackle_db::Row::field(self.row, other),
        }
    }
}

/// An object row: the row IS the picture, so it is its own thumbnail source
/// and its stem labels it when front matter has no title.
fn object_row(
    cfg: &Config,
    schemas: &crate::parts::Schemas,
    resolve_asset: &dyn Fn(&str) -> String,
    o: &crate::model::Row,
    thumbs: &crate::thumbs::Renditions,
) -> anyhow::Result<parts::PartMap> {
    let t = crate::thumbs::default_of(thumbs, &o.rel.to_string_lossy());
    let stem = o
        .rel
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    parts::from_row(
        cfg,
        schemas,
        resolve_asset,
        o,
        parts::Presentation {
            title: o.title.is_none().then_some(stem),
            hero: Some(t.map(|t| t.url.clone()).unwrap_or_else(|| o.url.clone())),
            dims: t.and_then(|t| t.dims),
            ..Default::default()
        },
        parts::FillOpts {
            thumbs: Some(thumbs),
            ..Default::default()
        },
    )
}

/// An image field's value as a URL. An absolute one names something outside
/// the site and is already a URL; anything else is a root-relative source
/// path and wears the baseurl.
pub(crate) fn is_absolute_url(s: &str) -> bool {
    s.contains("://") || s.starts_with("//")
}

pub(crate) fn asset_url(baseurl: &str, s: &str) -> String {
    // `s` is absolute in two ways, both of which must pass through untouched:
    // an external URL (`https://`, `//host`), or a site-root path the renderer
    // already resolved, a `/static/{hash}` thumb (baseurl-free by design,
    // thumbs.rs) or any leading-`/` ref. A source asset is root-RELATIVE
    // (`foo/bar.png`), so only those get the baseurl prefix. Without the
    // leading-`/` guard a resolved hero, `images(content)[0]`, read back from
    // already-rendered body HTML, becomes `//static/…` and 404s.
    if is_absolute_url(s) || s.starts_with('/') {
        s.to_string()
    } else {
        format!("{baseurl}/{s}")
    }
}

/// A content row as a listing member: prose when it has a body. `content` is
/// the body already truncated by the view's `summary` field, or `None`
/// where the caller shows no prose. Picture comes from computed `fields.hero`.
#[allow(clippy::too_many_arguments)]
fn content_row(
    cfg: &Config,
    schemas: &crate::parts::Schemas,
    resolve_asset: &dyn Fn(&str) -> String,
    view: &str,
    p: &crate::model::Row,
    thumbs: &crate::thumbs::Renditions,
    content: Option<String>,
    truncated: bool,
    blocks: Option<&[String]>,
) -> anyhow::Result<parts::PartMap> {
    parts::from_row(
        cfg,
        schemas,
        resolve_asset,
        p,
        parts::Presentation {
            content,
            truncated,
            ..Default::default()
        },
        parts::FillOpts {
            view: Some(view),
            blocks,
            thumbs: Some(thumbs),
        },
    )
}

/// The intro for one route: a grouped
/// route whose leaf value declares a record `intro` gets that value's
/// own prose, the course archive introduces the course, else the
/// view's intro applies to every partition.
pub(crate) fn route_intro(
    cfg: &Config,
    v: &View,
    view: &str,
    r: &Route,
    linkspace: &crate::links::LinkSpace,
    locale: &str,
) -> Result<Option<String>> {
    if r.key.is_some() {
        let chain = cfg.group_specs(view);
        if let Some(field) = chain.last() {
            if let Some(id) = crate::template::param(&r.params, field) {
                if let Some(i) = cfg.record(field, &id).and_then(|rec| rec.intro.as_ref()) {
                    let source = format!("record {field}.{id}: intro");
                    return render_config_prose(cfg, linkspace, locale, &source, i);
                }
            }
        }
    }
    match &v.intro {
        Some(i) => render_config_prose(cfg, linkspace, locale, &format!("view {view}: intro"), i),
        None => Ok(None),
    }
}

/// Config-authored prose (intros): markdown through the locale-aware
/// link resolver, `view:` links and source paths get the same strict
/// validation as any body; no browser-agreement bypass (config prose
/// has no directory).
fn render_config_prose(
    cfg: &Config,
    linkspace: &crate::links::LinkSpace,
    locale: &str,
    source: &str,
    text: &crate::config::LocalizedStr,
) -> Result<Option<String>> {
    let text = cfg.i18n_text(text, locale);
    let doc = crate::markdown::render_doc_with(text, &|form, href| {
        crate::links::resolve(
            cfg,
            linkspace,
            Path::new(""),
            "\u{0}",
            locale,
            source,
            form,
            href,
        )
    })?;
    Ok(Some(doc.whole.trim_end().to_string()))
}

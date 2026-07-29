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

/// The axis slot (q47, §6f): every axis THIS route is a member of, each a group
/// of member links with the current one flagged — the switcher a theme renders,
/// for a row page or a listing view alike. Supersedes the `translations`
/// relation: the locale axis is one group here.
///
/// The locale group comes from `by_logical` (a row's translation files) or the
/// same view route in other locales; a declared axis (theme, …) from the sibling
/// routes that differ in exactly that axis, other axes held at the current
/// member. A group with fewer than two members is no switcher and drops out.
pub(crate) fn axes_part(cfg: &Config, db: &SiteDb, r: &Route) -> Vec<parts::PartMap> {
    let default = cfg.i18n.default.as_str();
    let cur_locale = r.locale().unwrap_or(default);
    let mut groups = Vec::new();

    // The routes that are THIS page in another form: a row's own routes, or the
    // same view route (same group key and page).
    //
    // "Is this a view route" is the `view` column being non-empty (IO.md §3,
    // I13) — the three sites that mint one all set it, and nothing else does.
    // The `is_some` is not implied by the equality below: a route with no row
    // and no view is a shape this seam must not treat as a view's twin.
    let in_scope = |o: &Route| -> bool {
        match &r.row {
            Some(k) => o.row.as_ref() == Some(k),
            None => o.view.is_some() && o.view == r.view && o.key == r.key && o.page == r.page,
        }
    };

    // Locale axis. A row pivots through its translation files (by_logical); a
    // view through its own routes in other locales.
    let loc_members: Vec<(String, String, bool)> = if let Some(k) = &r.row {
        db.rows
            .get(k)
            .and_then(|p| db.by_logical.get(&p.logical))
            .into_iter()
            .flatten()
            .filter_map(|sk| db.rows.get(sk))
            .filter(|s| !s.url.is_empty())
            .map(|s| {
                (
                    cfg.i18n.name_of(s.locale()).to_string(),
                    s.url.clone(),
                    Some(&s.key) == r.row.as_ref(),
                )
            })
            .collect()
    } else {
        // Vary ONLY locale: hold the axis members fixed, or a view on another
        // axis would list its axis siblings as if they were translations.
        db.routes
            .iter()
            .filter(|o| in_scope(o) && o.axis == r.axis)
            .map(|o| {
                let loc = o.locale().unwrap_or(default);
                (
                    cfg.i18n.name_of(loc).to_string(),
                    o.url.clone(),
                    o.url == r.url,
                )
            })
            .collect()
    };
    if let Some(g) = parts::axis_group(
        "locale",
        cfg.i18n.string("translations", cur_locale),
        loc_members,
    ) {
        groups.push(g);
    }

    // Declared axes: pivot one, hold the rest (and locale) at the current member.
    for m in &r.axis {
        let Some(axis) = cfg.axes.get(&m.axis) else {
            continue;
        };
        let members: Vec<(String, String, bool)> = axis
            .values
            .iter()
            .filter_map(|v| {
                db.routes
                    .iter()
                    .find(|o| {
                        in_scope(o)
                            && o.locale() == r.locale()
                            && o.axis.len() == r.axis.len()
                            && r.axis.iter().all(|rm| {
                                let want = if rm.axis == m.axis { v } else { &rm.value };
                                o.axis
                                    .iter()
                                    .any(|om| om.axis == rm.axis && om.value == *want)
                            })
                    })
                    .map(|o| (v.clone(), o.url.clone(), v == &m.value))
            })
            .collect();
        if let Some(g) = parts::axis_group(&m.axis, &m.axis, members) {
            groups.push(g);
        }
    }
    groups
}

/// The collection at the base of a view's `from` chain — whose role (read off
/// its `source`, now that `kind` is gone) decides which render pass owns the
/// view's routes. None for a fold over every output, which has no collection
/// under it (IO.md §4).
pub(crate) fn view_base_collection<'a>(cfg: &'a Config, view: &str) -> Option<&'a Collection> {
    // A union's members share a role — they share a `from` vocabulary — so the
    // first answers for the whole base.
    let base = cfg.query(view).ok()?.base;
    cfg.collections.get(base.first()?)
}

/// The link resolver a page hands its slot fills (§6a): the fill's owner
/// directory is the relative base, and the consuming page's locale drives
/// view links — one nav.md serves every locale. The impossible `url_dir`
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

/// What an axis member sets, when it sets the field asked for (q53).
///
/// A member declares which row field its value stands in for — `theme` renders
/// one corpus several ways, `shell` gives a document its md twin — so a render
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
/// q32 settled that page URLs come from the owning view rather than a literal
/// copy in the producer, and this used to honour that by re-rendering the view's
/// route templates with `{n}`. It reads the view's already-materialized pages
/// instead, which is the same rule with one fewer way to be wrong — and it is
/// what lets a GROUPED view paginate (§5c). Re-rendering had two defects that
/// only a grouped-and-paginated route could show:
///
///   - the template also carries `{key}`, which a `{n}`-only renderer cannot
///     fill, so rendering failed outright;
///   - `total` counted every page of the view across ALL groups, so a
///     three-page partition would have offered three pages to every group in
///     it.
///
/// A materialized URL has neither problem: it already wears its group key, its
/// record slug (`{key}` is slugged in the URL and not in the params, so
/// re-rendering could disagree with the route it was naming) and its locale
/// prefix. Pages are only created where rows exist, so the sibling list is
/// exactly the pages there are.
pub(crate) fn pagination_parts(
    db: &SiteDb,
    _view: &str,
    _v: &View,
    r: &Route,
) -> Result<Option<parts::PartMap>> {
    let Some(cur) = r.page else { return Ok(None) };
    // Same view, same locale, same GROUP: pagination is per partition, and two
    // routes of one view are in the same partition when their group params
    // agree (empty for an ungrouped view, so it degenerates correctly).
    let mut siblings: Vec<&Route> = db
        .routes
        .iter()
        .filter(|x| {
            x.view == r.view && x.page.is_some() && x.locale() == r.locale() && x.params == r.params
        })
        .collect();
    siblings.sort_by_key(|x| x.page);
    let urls: Vec<String> = siblings.iter().map(|x| x.url.clone()).collect();
    Ok(parts::pagination(cur, &urls))
}

#[allow(clippy::too_many_arguments)]
/// Members of a view/route as previews — objects, truncated prose, or tree
/// bodies from `page_bodies` when the post map has none.
pub(crate) fn member_previews<'a>(
    cfg: &Config,
    db: &'a crate::model::SiteDb,
    view: &str,
    members: &[grackle_model::Key],
    thumbs: &crate::thumbs::Renditions,
    bodies: &std::collections::HashMap<&grackle_model::Key, crate::markdown::Doc>,
    page_bodies: &std::collections::HashMap<String, PageBody>,
    is_object: impl Fn(&grackle_model::Key) -> bool,
) -> Vec<parts::Preview<'a>> {
    let summary_field = cfg.fields_for(view).get("summary").and_then(|f| f.truncate);
    members
        .iter()
        .filter_map(|k| db.rows.get(k))
        .map(|p| {
            if is_object(&p.key) {
                return object_preview(p, thumbs);
            }
            let (html, truncated) = match bodies.get(&p.key) {
                Some(d) => match summary_field {
                    Some(t) => d.truncate(t.max_blocks, t.max_chars),
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
            row_preview(p, thumbs, Some(html), truncated)
        })
        .collect()
}

/// An object row as a preview: the row IS the picture, so it is its own
/// thumbnail source and its stem is the only label it has. `row` stays unset
/// — an object has no date, tags or prose to answer with.
pub(crate) fn object_preview<'a>(
    o: &crate::model::Row,
    thumbs: &crate::thumbs::Renditions,
) -> parts::Preview<'a> {
    let t = crate::thumbs::default_of(thumbs, &o.rel.to_string_lossy());
    parts::Preview {
        title: Some(
            o.rel
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default(),
        ),
        url: Some(o.url.clone()),
        src: Some(t.map(|t| t.url.clone()).unwrap_or_else(|| o.url.clone())),
        dims: t.and_then(|t| t.dims),
        ..Default::default()
    }
}

/// An image field's value as a URL. An absolute one names something outside
/// the site and is already a URL; anything else is a root-relative source
/// path and wears the baseurl.
pub(crate) fn is_absolute_url(s: &str) -> bool {
    s.contains("://") || s.starts_with("//")
}

pub(crate) fn asset_url(baseurl: &str, s: &str) -> String {
    if is_absolute_url(s) {
        s.to_string()
    } else {
        format!("{baseurl}/{s}")
    }
}

/// A row as a preview: everything the row can answer (§5e/q36).
///
/// Prose when it has a body, a picture when it has a hero, a note when it has
/// a description — the fragment takes what it wants and the hole algebra
/// deletes the rest. `content` is the body already truncated by the view's
/// `summary` field (§6d), or `None` where the caller shows no prose.
pub(crate) fn row_preview<'a>(
    p: &'a crate::model::Row,
    thumbs: &crate::thumbs::Renditions,
    content: Option<String>,
    truncated: bool,
) -> parts::Preview<'a> {
    let t = p
        .hero_source()
        .and_then(|s| crate::thumbs::default_of(thumbs, s));
    parts::Preview {
        row: Some(p),
        content,
        truncated,
        src: t.map(|t| t.url.clone()),
        dims: t.and_then(|t| t.dims),
        ..Default::default()
    }
}

/// The intro for one ROUTE (§6f enum records × q45 mode A): a grouped
/// route whose leaf value declares a record `intro` gets that value's
/// own prose — the course archive introduces the course — else the
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
        if let Some(field) = chain.last().map(|s| crate::model::spec_field(s)) {
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
/// link resolver — `view:` links and source paths get the same strict
/// validation as any body; no browser-agreement bypass (config prose
/// has no directory).
fn render_config_prose(
    cfg: &Config,
    linkspace: &crate::links::LinkSpace,
    locale: &str,
    source: &str,
    text: &crate::config::LocalizedStr,
) -> Result<Option<String>> {
    let text = cfg.i18n.text(text, locale);
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

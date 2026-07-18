//! Breadcrumb trails: where a row or a route sits in the site (§5c
//! provenance, §5h the landing chain).
//!
//! One family, five entry points, all answering the same question in
//! different currencies: `home_url` (the locale's root), `trail_root`
//! (Home, then the collection), `ancestors` (what the URL nests under),
//! `listing_title_and_trail` (a view route's own naming), `post_trail`
//! (a row's archive chain). Lifted out of `build.rs` unchanged — the
//! §9b round-2 audit named this a coherent module wanting out.

use anyhow::{Context, Result};

use crate::config::{Config, Kind, View};
use crate::db::{Post, Route, RouteKind, SiteDb};

/// The URL "Home" means for a locale (§6f): the locale's own homepage
/// when a translated index exists (`index.fr.html` → `/fr/`), else the
/// site root. Existence-checked, not assumed — a locale with translated
/// posts but no translated homepage keeps linking `/`.
pub fn home_url(cfg: &Config, db: &SiteDb, locale: &str) -> String {
    if locale != cfg.i18n.default {
        let prefixed = format!("/{locale}/");
        if db.pages.rows.iter().any(|p| p.rendered && p.url == prefixed) {
            return prefixed;
        }
    }
    "/".to_string()
}

/// Every trail roots the same way (§5c provenance): Home, then the
/// collection's own crumb, linked to its index. All three resolve per
/// locale (§6f): the engine's "home" string, the home URL (existence-
/// checked), the crumb's LocalizedStr, and the index URL locale-prefixed
/// — a French row's trail points at the French index, which exists
/// whenever French rows do (locale-parallel views are default-on; a
/// collection whose index view opted out keeps this honest only if its
/// rows opted out of translation too — `index` naming a VIEW instead of
/// a URL would close that, q32-adjacent, pending).
pub fn trail_root(
    cfg: &Config,
    db: &SiteDb,
    collection: &str,
    locale: &str,
) -> Vec<(String, Option<String>)> {
    let mut t =
        vec![(cfg.i18n.string("home", locale).to_string(), Some(home_url(cfg, db, locale)))];
    if let Some(col) = cfg.collections.get(collection) {
        if let (Some(c), Some(u)) = (&col.crumb, &col.index) {
            let u = if locale != cfg.i18n.default {
                format!("/{locale}{u}")
            } else {
                u.clone()
            };
            t.push((cfg.i18n.text(c, locale).to_string(), Some(u)));
        }
    }
    t
}

/// A listing route's title and provenance trail (§5c): the view's declared
/// `title`/`crumb` templates rendered over the route's group params — each
/// grouped *ancestor* linked to its own archive, this route's crumb as the
/// inert tail. This used to be a `match` on the layout kind re-deriving what
/// the config already knew; layout kinds are code, naming is the view's.
pub fn listing_title_and_trail(
    cfg: &Config,
    db: &SiteDb,
    view: &str,
    v: &View,
    r: &Route,
) -> Result<(String, Vec<(String, Option<String>)>)> {
    // Listings render at the view's locale (§6f): the route carries it
    // for locale-parallel materializations; absent = the default.
    let loc = r.locale.as_deref().unwrap_or(cfg.i18n.default.as_str());
    // §6f enum records: a grouped param renders its record's localized
    // NAME — "méta" on the French tag page, "Dinner" for a course —
    // while routes keep slugs and keys/params keep ids.
    let fields: Vec<String> = cfg
        .group_specs(view)
        .iter()
        .map(|s| crate::views::spec_field(s).to_string())
        .collect();
    let param = |k: &str| -> Option<String> {
        let raw = crate::route::param(&r.params, k)?;
        let field = if k == "key" {
            fields.last().map(String::as_str)
        } else if fields.iter().any(|f| f == k) {
            Some(k)
        } else {
            None
        };
        match field {
            Some(f) => Some(cfg.record_name(f, &raw, loc).to_string()),
            None => Some(raw),
        }
    };
    let text = |t: &crate::config::LocalizedStr| cfg.i18n.text(t, loc).to_string();
    let title = match &v.title {
        Some(t) => crate::route::render(&text(t), param)
            .with_context(|| format!("view {view}: title"))?,
        None => r.key.clone().unwrap_or_else(|| view.to_string()),
    };
    let tail = match r.page {
        // Paginated trails keep the engine's `page` string for now — crumb
        // templates for paginated views are punted with open question 30
        // (pagination × subdivision).
        Some(p) => {
            (p > 1).then(|| cfg.i18n.string("page", loc).replace("{n}", &p.to_string()))
        }
        None => {
            let tmpl = v.crumb.as_ref().or(v.title.as_ref());
            match tmpl {
                Some(t) => Some(crate::route::render(&text(t), param)
                    .with_context(|| format!("view {view}: crumb"))?),
                None => r.key.clone(),
            }
        }
    };
    let mut trail = trail_root(cfg, db, &cfg.query(view)?.base, loc);
    // The landing chain for listings (q45): URL ancestors between the
    // root and this route are crumbs too — /recipes/courses/dinner/
    // climbs through the /recipes/ landing. Deduped by URL, because
    // the collection crumb already roots /blog/-style listings there.
    for (url, label) in ancestors(cfg, db, &r.url) {
        if trail.iter().any(|(_, u)| u.as_deref() == Some(url.as_str())) {
            continue;
        }
        trail.push((label, Some(url)));
    }
    for anc in cfg.grouped_chain(view).iter().filter(|n| *n != view) {
        let av = &cfg.views[anc.as_str()];
        let tmpl = av.crumb.as_ref().or(av.title.as_ref());
        if let (Some(t), Some(route_t)) = (tmpl, av.route.as_deref()) {
            let label = crate::route::render(&text(t), param)
                .with_context(|| format!("view {anc}: crumb"))?;
            let url = crate::route::render(route_t, param)?;
            trail.push((label, Some(url)));
        }
    }
    if let Some(t) = tail {
        trail.push((t, None));
    }
    Ok((title, trail))
}

/// A post's breadcrumb trail: the shared root, then the collection's
/// declared `trail` view chain rendered with the post's own group keys —
/// each level linked to its archive — ending in the inert day. All
/// provenance (§5c); the only special case left is drafts, which wait on
/// the profiles work (§4a).
pub fn post_trail(cfg: &Config, db: &SiteDb, p: &Post) -> Vec<(String, Option<String>)> {
    // The posts collection, whatever it is named (§7a: the example's is
    // `notes`). One posts table means one posts collection today.
    let col = cfg.collections.iter().find(|(_, c)| c.kind == Kind::Posts);
    let loc = p.locale.as_str();
    let mut t = match &col {
        Some((name, _)) => trail_root(cfg, db, name, loc),
        None => {
            vec![(cfg.i18n.string("home", loc).to_string(), Some(home_url(cfg, db, loc)))]
        }
    };
    if p.draft {
        t.push((cfg.i18n.string("drafts", loc).to_string(), Some("/drafts".to_string())));
        t.push((p.title.clone(), None));
        return t;
    }
    let trail_view = col.and_then(|(_, c)| c.trail.as_deref());
    let mut chained = false;
    if let Some(trail_view) = trail_view {
        for name in cfg.grouped_chain(trail_view) {
            let Some(v) = cfg.views.get(&name) else { continue };
            let specs = cfg.group_specs(&name);
            let combos = crate::views::key_combos(p, &specs);
            let Some(combo) = combos.first() else { break }; // undated: no trail
            let params: Vec<(String, String)> =
                combo.iter().flat_map(|k| k.params.clone()).collect();
            let get = |k: &str| crate::route::param(&params, k);
            let tmpl = v.crumb.as_ref().or(v.title.as_ref());
            if let (Some(tm), Some(rt)) = (tmpl, v.route.as_deref()) {
                let tm = cfg.i18n.text(tm, loc);
                if let (Ok(label), Ok(url)) =
                    (crate::route::render(tm, get), crate::route::render(rt, get))
                {
                    t.push((label, Some(url)));
                    chained = true;
                }
            }
        }
    }
    // The inert tail: a bare day only reads after year › month crumbs;
    // with no archive chain declared, it dangled as a naked "10" — the
    // whole date is the honest crumb there.
    if let Some(d) = p.date {
        let tail = if chained {
            d.format("%-d").to_string()
        } else {
            crate::db::pretty_date(d)
        };
        t.push((tail, None));
    }
    t
}

/// Ancestor pages of a URL, outermost first — the tree relation from §5a.
///
/// Walks the URL upward and keeps the levels that are themselves rendered
/// pages, which is what `breadcrumb.rb` did by scanning every page for a
/// matching url. Here it is a lookup, because the tree is indexed.
pub fn ancestors(cfg: &Config, db: &SiteDb, url: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut cur = url.trim_end_matches('/');
    while let Some(i) = cur.rfind('/') {
        cur = &cur[..i];
        if cur.is_empty() {
            break;
        }
        let parent = format!("{cur}/");
        // §6f/q45: the locale prefix makes the homepage look like a
        // directory ancestor of every /fr/… URL — but Home is the trail
        // root's job, and it was duplicating (`Accueil › Carnet de
        // terrain › …`).
        if cfg.i18n.locales.iter().any(|l| parent == format!("/{l}/")) {
            continue;
        }
        if let Some(p) = db.pages.rows.iter().find(|p| p.url == parent && p.rendered) {
            if let Some(t) = &p.title {
                out.push((parent, t.clone()));
            }
        } else if let Some(r) = db.routes.iter().find(|r| {
            r.kind == RouteKind::View
                && r.url == parent
                && r.key.is_none()
                && r.page.is_none_or(|n| n == 1)
        }) {
            // q45, the landing chain's first slice: a materialized landing
            // above this URL is an ancestor like any index page — /books/
            // lists the books, so a book's trail climbs through it. The
            // crumb is the view's own (crumb, else title), resolved at the
            // route's locale; a mode-B landing never reaches here (its
            // claimed row matched above, and the row's title wins).
            if let Some(v) = r.view.as_deref().and_then(|n| cfg.views.get(n)) {
                if let Some(t) = v.crumb.as_ref().or(v.title.as_ref()) {
                    let loc = r.locale.as_deref().unwrap_or(&cfg.i18n.default);
                    out.push((parent, cfg.i18n.text(t, loc).to_string()));
                }
            }
        }
    }
    out.reverse();
    out
}

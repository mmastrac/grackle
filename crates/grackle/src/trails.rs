//! Breadcrumb trails: where a row or a route sits in the site (§5c
//! provenance, §5h the landing chain).
//!
//! One family, five entry points, all answering the same question in
//! different currencies: `home_url` (the locale's root), `trail_root`
//! (Home), `ancestors` (what the URL nests under),
//! `listing_title_and_trail` (a view route's own naming), `post_trail`
//! (a row's archive chain).
//!
//! There are exactly two producers of a crumb, and the split is the point
//! (q46): the **URL climb** supplies every level the path nests under, and
//! a collection's declared `trail` view supplies the subdivision chain,
//! which renders from a row's own group keys and so cannot be recovered
//! from the path at all. Nothing is stated twice.

use anyhow::{Context, Result};

use crate::config::{Config, View};
use crate::db::{Route, Row, SiteDb};

/// The URL "Home" means for a locale (§6f): the locale's own homepage
/// when a translated index exists (`index.fr.html` → `/fr/`), else the
/// site root. Existence-checked, not assumed — a locale with translated
/// posts but no translated homepage keeps linking `/`.
pub fn home_url(cfg: &Config, db: &SiteDb, locale: &str) -> String {
    if locale != cfg.i18n.default {
        let prefixed = format!("/{locale}/");
        if db.rows.iter().any(|p| p.rendered && p.url == prefixed) {
            return prefixed;
        }
    }
    "/".to_string()
}

/// Every trail roots the same way (§5c provenance): Home, resolved per
/// locale (§6f) — the engine's "home" string, and a home URL that is
/// existence-checked rather than assumed.
///
/// Home is *all* the root is (q46, §5h): every crumb between it and the
/// current page comes from climbing the URL through [`ancestors`], so a
/// collection never names itself. `/fr/blog/` is found that way rather
/// than built by string-prefixing a configured index.
pub fn trail_root(cfg: &Config, db: &SiteDb, locale: &str) -> Vec<(String, Option<String>)> {
    vec![(
        cfg.i18n.string("home", locale).to_string(),
        Some(home_url(cfg, db, locale)),
    )]
}

/// A listing route's title and provenance trail (§5c): the view's declared
/// `title`/`crumb` templates rendered over the route's group params — each
/// grouped *ancestor* linked to its own archive, this route's crumb as the
/// inert tail. Naming is the view's, not the layout kind's.
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
        .map(|s| crate::db::spec_field(s).to_string())
        .collect();
    let param = |tok: &str| -> Option<String> {
        // Bare and `group:`-qualified tokens name the same group param; no other
        // namespace is in scope in a listing trail.
        let (ns, k) = crate::template::classify(tok);
        if matches!(ns, Some(n) if n != "group") {
            return None;
        }
        let raw = crate::template::param(&r.params, k)?;
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
        Some(t) => crate::template::render(&text(t), param)
            .with_context(|| format!("view {view}: title"))?,
        // The empty-title fallback resolves through the i18n string layer keyed
        // on the view name — the door the base's `@home`/`@blog` routes reach
        // (§4d) — so an engine view resolves its built-in and any other resolves
        // to `""`, collapsing the heading rather than leaking the config key. A
        // grouped archive keeps its group key.
        None => r
            .key
            .clone()
            .unwrap_or_else(|| cfg.i18n.string(view, loc).to_string()),
    };
    let tail = match r.page {
        // Paginated trails keep the engine's `page` string for now — crumb
        // templates for paginated views are punted with open question 30
        // (pagination × subdivision). Page *one* is not a page-of, though:
        // it is the view's root, so it names itself in the tail the way
        // every other listing does.
        Some(p) if p > 1 => Some(cfg.i18n.string("page", loc).replace("{n}", &p.to_string())),
        _ => {
            let tmpl = v.crumb.as_ref().or(v.title.as_ref());
            match tmpl {
                Some(t) => Some(
                    crate::template::render(&text(t), param)
                        .with_context(|| format!("view {view}: crumb"))?,
                ),
                None => r.key.clone(),
            }
        }
    };
    let mut trail = trail_root(cfg, db, loc);
    // The landing chain (q45): URL ancestors between the root and this
    // route are crumbs — /recipes/courses/dinner/ climbs through the
    // /recipes/ landing, /blog/tags/rust/ through /blog/.
    for (url, label) in ancestors(cfg, db, &r.url) {
        trail.push((label, Some(url)));
    }
    for anc in cfg.grouped_chain(view).iter().filter(|n| *n != view) {
        let av = &cfg.views[anc.as_str()];
        let tmpl = av.crumb.as_ref().or(av.title.as_ref());
        if let (Some(t), Some(route_t)) = (tmpl, av.route.as_deref()) {
            let label = crate::template::render(&text(t), param)
                .with_context(|| format!("view {anc}: crumb"))?;
            let url = crate::template::render(route_t, param)?;
            trail.push((label, Some(url)));
        }
    }
    if let Some(t) = tail {
        trail.push((t, None));
    }
    Ok((title, trail))
}

/// A post's breadcrumb trail: Home, the landings its URL nests under, then
/// the collection's declared `trail` view chain rendered with the post's
/// own group keys — each level linked to its archive — ending in the inert
/// day. All provenance (§5c) — no special cases: a draft trails like any
/// other row, because a profile decides whether it is selected at all
/// (§4a), and its address is the profile's `baseurl`, not a literal the
/// trail builder carries.
///
/// The two walks divide cleanly: [`ancestors`] matches only *ungrouped*
/// view roots, so `/blog/2022/12/16/x.html` finds the `/blog/` landing and
/// steps straight past the year and month archives — leaving them to
/// `trail`, whose subdivision chain is genuinely non-derivable from the
/// URL (it renders each level from the post's own group keys, not from
/// path segments).
pub fn post_trail(cfg: &Config, db: &SiteDb, p: &Row) -> Vec<(String, Option<String>)> {
    let loc = p.locale.as_str();
    let mut t = trail_root(cfg, db, loc);
    for (url, label) in ancestors(cfg, db, &p.url) {
        t.push((label, Some(url)));
    }
    // The posts collection that declares a trail, whatever it is named
    // (§7a: the example's is `notes`). Keyed on the DECLARATION, not on
    // being first: `_posts` and `_drafts` are both posts collections.
    let trail_view = cfg
        .collections
        .values()
        .filter(|c| c.is_posts())
        .find_map(|c| c.trail.as_deref());
    let mut chained = false;
    if let Some(trail_view) = trail_view {
        for name in cfg.grouped_chain(trail_view) {
            let Some(v) = cfg.views.get(&name) else {
                continue;
            };
            let specs = cfg.group_specs(&name);
            let combos = crate::views::key_combos(p, &specs);
            let Some(combo) = combos.first() else { break }; // undated: no trail
            let params: Vec<(String, String)> =
                combo.iter().flat_map(|k| k.params.clone()).collect();
            // Bare or `group:`-qualified — one group namespace in scope here.
            let get = |tok: &str| {
                let (ns, k) = crate::template::classify(tok);
                match ns {
                    None | Some("group") => crate::template::param(&params, k),
                    _ => None,
                }
            };
            let tmpl = v.crumb.as_ref().or(v.title.as_ref());
            if let (Some(tm), Some(rt)) = (tmpl, v.route.as_deref()) {
                let tm = cfg.i18n.text(tm, loc);
                if let (Ok(label), Ok(url)) = (
                    crate::template::render(tm, get),
                    crate::template::render(rt, get),
                ) {
                    t.push((label, Some(url)));
                    chained = true;
                }
            }
        }
    }
    // The inert tail: a bare day only reads after year › month crumbs, so
    // with no archive chain declared the whole date is the honest crumb.
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
        // root's job, so skip it here or it doubles.
        if cfg.i18n.locales.iter().any(|l| parent == format!("/{l}/")) {
            continue;
        }
        if let Some(p) = db
            .by_url
            .get(parent.as_str())
            .and_then(|k| db.rows.get(k))
            .filter(|p| p.rendered)
        {
            if let Some(t) = &p.title {
                out.push((parent, t.clone()));
            }
        } else if let Some(r) = db.routes.iter().find(|r| {
            // "Is this a view route" is the `view` column being non-empty
            // (IO.md §3, I13).
            r.view.is_some()
                && r.url == parent
                // The view's ROOT route, not one of its grouped archives:
                // group keys accumulate in `params` along the subdivision
                // chain, so empty means ungrouped. Not `key`, which a
                // paginated view stamps with a synthetic `"page 1"` on its
                // first route — that would hide `/blog/` from the climb.
                && r.params.is_empty()
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

#[cfg(test)]
// The climb and the declared trail moved to a fixture test (§7d): both
// faked what a real paginated route stamps. See
// `crates/grackle/tests/fixtures/crumb-trails`.
mod tests {
    use super::*;
}

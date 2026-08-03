//! Pre-emit setup: icon, themes, thumbs, backlinks, alternates.

use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::config::Config;
use crate::markdown::Doc;
use crate::model::{Ask, Route, RouteKind, SiteDb};
use crate::passes::preview::{is_absolute_url, view_base_collection};
use crate::pipeline::bodies::row_body_html;
use crate::pipeline::postpass::cited_urls_cited;
use crate::pipeline::types::{Backlink, PageBody, SiteOutput, Stats};
use crate::store::split_front_matter;
use crate::tags;
use crate::theme;

/// A row's OTHER axis forms as `rel="alternate"` (q53). One entry per member
/// route of the same row that is NOT this route — a member points at its
/// siblings, `rel="canonical"` names the one that counts. A form whose URL
/// extension is in `[media_types]` (the md twin) carries that `type`; a
/// same-format restyle (a theme member) carries none, being the same
/// representation elsewhere.
///
/// Empty for a row on no axis, so a page with one form announces nothing.
pub(crate) fn axis_alternates(
    db: &SiteDb,
    site_url: &str,
    r: &Route,
    media_types: &std::collections::BTreeMap<String, String>,
) -> Vec<crate::model::Alternate> {
    let Some(row) = &r.row else {
        return Vec::new();
    };
    if r.axis.is_empty() {
        return Vec::new();
    }
    db.routes
        .iter()
        .filter(|o| o.row.as_ref() == Some(row) && o.url != r.url)
        .map(|o| crate::model::Alternate {
            href: format!("{site_url}{}", o.url),
            hreflang: None,
            // `type` only when the form's media type is not the ordinary HTML —
            // then the alternate is a genuinely different representation.
            media_type: alt_media_type(&o.url, media_types),
        })
        .collect()
}

/// The media type a member URL advertises as a `rel="alternate"` `type`, or
/// `None` for an ordinary HTML page (a restyle names no type — it is the same
/// representation). Keyed off the URL's extension against `[media_types]`.
pub(crate) fn alt_media_type(
    url: &str,
    media_types: &std::collections::BTreeMap<String, String>,
) -> Option<String> {
    let ext = url.rsplit('/').next()?.rsplit_once('.')?.1;
    media_types.get(ext).cloned()
}

/// The site-icon candidates, in the order a browser should prefer them (§4d).
/// SVG leads because it is the one that scales; `.ico` stays in the list even
/// though browsers probe `/favicon.ico` on their own, because a site whose
/// only icon is an `.ico` should still say so out loud.
const ICON_URLS: &[&str] = &[
    "/favicon.svg",
    "/favicon.png",
    "/favicon.ico",
    "/favicon.webp",
    "/favicon.gif",
];

/// The site icon: the first candidate a row occupies, under `baseurl` like
/// every other published asset. Empty when the tree has none, and empty is
/// what makes every consumer drop its tag.
///
/// **A URL convention, not a filename lookup**, which is why this needs no
/// config key. An icon that lives somewhere else in the tree is pinned with an
/// ordinary named object route (§4) — `match = "brand/icon-v3.png"`,
/// `route = "/favicon.png"` — and this finds it at the URL, not the path.
///
/// Nothing here publishes the row. The `<link>` this feeds is a citation, and
/// `materialize_referenced` publishes what the chrome cites — the case its own
/// doc comment already named.
pub(crate) fn site_icon(cfg: &Config, db: &SiteDb) -> String {
    ICON_URLS
        .iter()
        .find(|u| db.row_by_url(u).is_some())
        .map(|u| format!("{}{u}", cfg.site.baseurl))
        .unwrap_or_default()
}

/// Declared theme names vs the registry (MERGE.md C2), before render.
/// Name half only — subtheme tokens are CSS fodder. Themeless rows skip
/// (site default already checked in `load_all`).
pub(crate) fn check_theme_names(cfg: &Config, db: &SiteDb, themes: &theme::Themes) -> Result<()> {
    for (name, axis) in &cfg.axes {
        if axis.field != "theme" {
            continue;
        }
        for value in &axis.values {
            themes
                .get(Some(theme::split_spec(value).0))
                .with_context(|| format!("[axes.{name}] values: {value:?}"))?;
        }
    }
    for (name, v) in &cfg.views {
        if let Some(spec) = v.theme.as_deref() {
            themes
                .get(Some(theme::split_spec(spec).0))
                .with_context(|| format!("view {name}: theme = {spec:?}"))?;
        }
    }
    for row in db.rows.iter() {
        if let Some(spec) = row.theme.as_deref() {
            themes
                .get(Some(theme::split_spec(spec).0))
                .with_context(|| format!("{}: theme = {spec:?}", row.path.display()))?;
        }
    }
    Ok(())
}

/// **Renditions** (§6b, IO.md §4a): collect the DEMAND, run the transform once
/// per distinct ask, publish under `/static/`, and hand the render passes a map
/// from ask → output so each citation reaches the rendition it asked for.
///
/// The asks come from citations, which is the model's whole claim about
/// renditions: `{% image %}` in post bodies and in rendered page bodies alike
/// (`code/legacy/*` pages use the tag too), image-typed schema fields, and the
/// members a gallery arranges. Nothing is declared and nothing is eager — an
/// image nothing cites gets no rendition, and an image two pages cite at two
/// widths gets two.
///
/// The cache is content-addressed by the same law the address is (input bytes +
/// parameters), so a warm build only reads and hashes each source; a cold one
/// decodes, resizes and re-encodes.
pub(crate) fn thumbs_pass(
    cfg: &Config,
    db: &SiteDb,
    root: &Path,
    out_map: &mut SiteOutput,
    stats: &mut Stats,
) -> Result<crate::thumbs::Renditions> {
    let mut asks: Vec<Ask> = Vec::new();
    for p in db.posts() {
        asks.extend(tags::image_asks(&crate::store::read_body(&p.path)?));
    }
    // Image-typed schema fields (§5b) — covers and the like — render too:
    // they are what heroes and cards show (q23). No tag wrote these, so they
    // ask for the engine's default rendition.
    for p in db.pages() {
        // An absolute url names something outside the site (load.rs leaves it
        // alone for the same reason): there is no file here to transform.
        asks.extend(
            p.images
                .values()
                .filter(|s| !is_absolute_url(s))
                .map(|s| Ask::thumb(s.clone())),
        );
    }
    for r in &db.routes {
        // `Page` here means "renders, and its body was not already scanned by
        // the posts loop above" — the second half is what keeps this a `kind`
        // test after I13. `p.rendered` alone would re-read every post; moving
        // the scan onto rows instead would change WHICH rows are scanned (a
        // claimed row has no route, so it is not scanned today), which is a
        // behaviour change and not this item's.
        if r.kind == RouteKind::Page {
            if let Some(src) = &r.source {
                if let Ok(text) = std::fs::read_to_string(src) {
                    let (_, _, body) = split_front_matter(&text);
                    asks.extend(tags::image_asks(body));
                }
            }
        }
        // Gallery members (object-backed views) render too — the gallery pass
        // shows renditions and links originals, same as {% image %}.
        if let Some(view) = &r.view {
            if view_base_collection(cfg, view).is_some_and(|c| c.is_objects()) {
                for k in &r.members {
                    if let Some(o) = db.rows.get(k) {
                        asks.push(Ask::thumb(o.rel.to_string_lossy().to_string()));
                    }
                }
            }
        }
    }
    let cache_dir = root.join("_cache/thumbs");
    let thumbs = crate::thumbs::generate(root, &cache_dir, &cfg.site.baseurl, &asks)?;
    let mut published: HashSet<String> = HashSet::new();
    for t in thumbs.values() {
        // Two asks that produced identical bytes share one address and one
        // artifact — the untransformed-twin rule, one transform along.
        if published.insert(t.address.clone()) {
            let bytes = std::fs::read(&t.cache_path)
                .with_context(|| format!("reading thumb {}", t.cache_path.display()))?;
            out_map.insert(t.address.clone(), bytes);
            stats.thumbs += 1;
        }
    }
    Ok(thumbs)
}

/// The link graph, both directions, from one scan. `linked_from` inverts the
/// citations (who points here); `links_to` keeps them forward (where this
/// points) — the derived relation names §6g's `related` default reads to
/// avoid re-showing a page you already linked. Both are the *citation* view of
/// the graph: a listing's spliced arrangement links are not citations (§6g
/// Problem 2), so both skip the splice — unlike on-demand publishing, which
/// scans with `cited_urls` directly and must still see them.
pub(crate) fn backlinks_map(
    db: &SiteDb,
    bodies: &HashMap<&grackle_db::Key, Doc>,
    page_bodies: &HashMap<String, PageBody>,
    site_url: &str,
) -> (HashMap<String, Vec<Backlink>>, HashMap<String, Vec<String>>) {
    // `rendered` is true for every post, so one predicate serves both.
    let is_target: HashSet<&str> = db
        .rows
        .iter()
        .filter(|p| p.rendered)
        .map(|p| p.url.as_str())
        .collect();

    // The axis is legitimately mixed: an undated row is allowed. Only the body
    // map differs by origin — posts hold their body, pages are re-read.
    let mut sources: Vec<(&str, String, Option<chrono::NaiveDate>, &str)> = Vec::new();
    for p in &db.rows {
        if let Some(html) = row_body_html(p, bodies, page_bodies) {
            sources.push((
                p.url.as_str(),
                p.title.clone().unwrap_or_default(),
                p.as_date("date"),
                html,
            ));
        }
    }

    let mut map: HashMap<String, Vec<Backlink>> = HashMap::new();
    let mut links_to: HashMap<String, Vec<String>> = HashMap::new();
    for (src_url, title, date, html) in sources {
        let mut seen: HashSet<String> = HashSet::new();
        for t in cited_urls_cited(html, src_url, site_url) {
            if t != src_url && is_target.contains(t.as_str()) && seen.insert(t.clone()) {
                map.entry(t.clone())
                    .or_default()
                    .push((title.clone(), src_url.to_string(), date));
                links_to.entry(src_url.to_string()).or_default().push(t);
            }
        }
    }
    // Newest citation first, undated last — the same ordering `order` gives
    // the posts table, so a reader meets both lists the same way.
    for v in map.values_mut() {
        v.sort_by(|a, b| {
            b.2.cmp(&a.2)
                .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
                .then_with(|| a.1.cmp(&b.1))
        });
    }
    (map, links_to)
}

#[cfg(test)]
mod alternates_tests {
    use super::*;
    use std::collections::BTreeMap;

    fn base_types() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("md".into(), "text/markdown".into()),
            ("xml".into(), "application/xml".into()),
            ("json".into(), "application/json".into()),
            ("txt".into(), "text/plain".into()),
            ("png".into(), "image/png".into()),
            ("jpg".into(), "image/jpeg".into()),
            ("jpeg".into(), "image/jpeg".into()),
            ("gif".into(), "image/gif".into()),
            ("webp".into(), "image/webp".into()),
            ("svg".into(), "image/svg+xml".into()),
            ("ico".into(), "image/x-icon".into()),
            ("avif".into(), "image/avif".into()),
        ])
    }

    #[test]
    fn alt_media_type_names_only_non_html_forms() {
        let types = base_types();
        assert_eq!(
            alt_media_type("/notes/one.md", &types),
            Some("text/markdown".into())
        );
        assert_eq!(
            alt_media_type("/feed.xml", &types),
            Some("application/xml".into())
        );
        assert_eq!(
            alt_media_type("/static/cover.jpg", &types),
            Some("image/jpeg".into())
        );
        assert_eq!(
            alt_media_type("/favicon.ico", &types),
            Some("image/x-icon".into())
        );
        // A restyle at a directory URL is the same representation — no type.
        assert_eq!(alt_media_type("/ledger/notes/one/", &types), None);
        assert_eq!(alt_media_type("/page.html", &types), None);
    }
}

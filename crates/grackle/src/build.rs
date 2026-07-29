//! Render the database to a set of URL → bytes outputs (DESIGN.md §7).
//!
//! `render_site` produces the whole site in memory, keyed by URL. Both clients
//! consume it: `build` writes the map to disk (AOT), and `serve` holds it
//! resident and answers requests from it — the "no output directory in dev"
//! the design calls for. One render path, two materializations.

use anyhow::{bail, Context, Result};
use rayon::prelude::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::assemble::chain;
use crate::config::{Collection, Config, View};
use crate::db::{Rendition, Route, RouteKind, Row, SiteDb};
use crate::markdown::Doc;
use crate::parts;
use crate::render::{self, Site, Theme};
use crate::store::split_front_matter;
use crate::tags;
use crate::theme;

/// The rendered site, keyed by URL (`/blog/`, `/atom.xml`, `/css/main.css`,
/// `/static/{hash}.jpg`, …). A directory URL ends in `/` and, on disk, becomes
/// that directory's `index.html`.
pub type SiteOutput = BTreeMap<String, Vec<u8>>;

#[derive(Default)]
pub struct Stats {
    pub posts: usize,
    pub pages: usize,
    pub listings: usize,
    pub copied: usize,
    pub css: usize,
    pub serialized: usize,
    /// Distinct derived thumbnails published under `/static/`.
    pub thumbs: usize,
    /// Rows published because something referenced them (§4 on-demand).
    pub on_demand: usize,
    pub skipped: Vec<String>,
    /// Posts whose embeddings are missing or stale (§6b). The caller decides
    /// when to run the model: `build` before rendering, `serve` in the
    /// background with a re-render on completion.
    pub embed_pending: Vec<crate::embed::Pending>,
    pub search_bytes: usize,
    /// Stylesheets that failed to compile. Collected rather than thrown so
    /// the two callers can disagree: `serve` prints and keeps the loop
    /// alive, `build` refuses to publish. A site whose CSS silently failed
    /// looks deployable and is wrong, which is the one outcome worth
    /// failing a build over.
    pub css_errors: Vec<String>,
    /// CSS complaints that are not failures — today, the one about a
    /// `_tokens.scss` nothing imports. Printed as they happen (`serve` shows
    /// them on every rebuild); collected here only so a test can assert on
    /// SILENCE, which is what a warning that lies is fixed into. Nothing
    /// reads this to decide anything.
    pub css_warnings: Vec<String>,
}

/// A URL ending in `/` is served as that directory's index.html.
fn out_path(out: &Path, url: &str) -> PathBuf {
    let rel = url.trim_start_matches('/');
    if url.ends_with('/') || rel.is_empty() {
        out.join(rel).join("index.html")
    } else {
        out.join(rel)
    }
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(path, bytes).with_context(|| format!("writing {}", path.display()))
}

/// q53: the locale axis as head alternates. Every version lists every
/// version, ITSELF INCLUDED, which is what `hreflang` asks for — a page
/// that omits itself is a common and quiet mistake.
///
/// Empty when the row has no twins, so a monolingual site emits nothing.
fn locale_alternates(
    site_url: &str,
    self_locale: &str,
    self_url: &str,
    twins: &[(String, String)],
) -> Vec<render::Alternate> {
    if twins.is_empty() {
        return Vec::new();
    }
    let one = |loc: &str, url: &str| render::Alternate {
        href: format!("{site_url}{url}"),
        hreflang: Some(loc.to_string()),
        media_type: None,
    };
    let mut v = vec![one(self_locale, self_url)];
    v.extend(twins.iter().map(|(loc, url)| one(loc, url)));
    v
}

/// A row's OTHER axis forms as `rel="alternate"` (q53). One entry per member
/// route of the same row that is NOT this route — a member points at its
/// siblings, `rel="canonical"` names the one that counts. A form whose URL has a
/// non-HTML media type (the md twin) carries that `type`; a same-format restyle
/// (a theme member) carries none, being the same representation elsewhere.
///
/// Empty for a row on no axis, so a page with one form announces nothing.
fn axis_alternates(db: &SiteDb, site_url: &str, r: &Route) -> Vec<render::Alternate> {
    let Some(row) = &r.row else {
        return Vec::new();
    };
    if r.axis.is_empty() {
        return Vec::new();
    }
    db.routes
        .iter()
        .filter(|o| o.row.as_ref() == Some(row) && o.url != r.url)
        .map(|o| render::Alternate {
            href: format!("{site_url}{}", o.url),
            hreflang: None,
            // `type` only when the form's media type is not the ordinary HTML —
            // then the alternate is a genuinely different representation.
            media_type: alt_media_type(&o.url),
        })
        .collect()
}

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
    let cur_locale = r.locale.as_deref().unwrap_or(default);
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
                    cfg.i18n.name_of(&s.locale).to_string(),
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
                let loc = o.locale.as_deref().unwrap_or(default);
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
                            && o.locale == r.locale
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

/// The media type a member URL advertises as a `rel="alternate"` `type`, or
/// `None` for an ordinary HTML page (a restyle names no type — it is the same
/// representation). Keyed off the URL's extension, the one thing that says a
/// form is a different format.
fn alt_media_type(url: &str) -> Option<String> {
    let ext = url.rsplit('/').next()?.rsplit_once('.')?.1;
    match ext {
        "md" => Some("text/markdown".to_string()),
        "xml" => Some("application/xml".to_string()),
        "json" => Some("application/json".to_string()),
        "txt" => Some("text/plain".to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod alternates_tests {
    use super::*;

    #[test]
    fn a_row_with_no_twins_announces_nothing() {
        assert!(locale_alternates("https://s", "en", "/a/", &[]).is_empty());
    }

    #[test]
    fn every_version_lists_itself_and_its_twins() {
        let twins = vec![("fr".to_string(), "/fr/a/".to_string())];
        let alts = locale_alternates("https://s", "en", "/a/", &twins);
        let got: Vec<(Option<String>, String)> = alts
            .iter()
            .map(|a| (a.hreflang.clone(), a.href.clone()))
            .collect();
        assert_eq!(
            got,
            vec![
                (Some("en".to_string()), "https://s/a/".to_string()),
                (Some("fr".to_string()), "https://s/fr/a/".to_string()),
            ]
        );
        assert!(alts.iter().all(|a| a.media_type.is_none()));
    }

    #[test]
    fn alt_media_type_names_only_non_html_forms() {
        assert_eq!(
            alt_media_type("/notes/one.md"),
            Some("text/markdown".into())
        );
        assert_eq!(alt_media_type("/feed.xml"), Some("application/xml".into()));
        // A restyle at a directory URL is the same representation — no type.
        assert_eq!(alt_media_type("/ledger/notes/one/"), None);
        assert_eq!(alt_media_type("/page.html"), None);
    }
}

/// Write a rendered site to a directory (AOT). Thin wrapper over `render_site`.
pub fn build(cfg: &Config, db: &mut SiteDb, out: &Path) -> Result<Stats> {
    // AOT builds publish, so they wait for fresh embeddings: bring the cache
    // current first, then render once with nothing pending.
    let cache = cfg.root().join("_cache/embeddings");
    if let Ok(l) = crate::embed::load(db, &cache) {
        if !l.pending.is_empty() {
            println!("grackle: embedding {} changed posts…", l.pending.len());
            if let Err(e) = crate::embed::embed_pending(&cache, &l.pending) {
                eprintln!("grackle: embedding failed, using stale/absent vectors: {e:#}");
            }
        }
    }
    let (map, stats) = render_site(cfg, db)?;
    // A publishing build refuses a stylesheet that did not compile. `serve`
    // makes the opposite call on the same data — see `Stats::css_errors`.
    // Nothing is written when this fires, so a failed build leaves the last
    // good output in place.
    if !stats.css_errors.is_empty() {
        anyhow::bail!(
            "{} stylesheet(s) failed to compile; refusing to publish:\n  {}",
            stats.css_errors.len(),
            stats.css_errors.join("\n  ")
        );
    }
    let _ = std::fs::remove_dir_all(out);
    std::fs::create_dir_all(out)?;
    for (url, bytes) in &map {
        write(&out_path(out, url), bytes)?;
    }
    Ok(stats)
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
fn site_icon(cfg: &Config, db: &SiteDb) -> String {
    ICON_URLS
        .iter()
        .find(|u| db.row_by_url(u).is_some())
        .map(|u| format!("{}{u}", cfg.site.baseurl))
        .unwrap_or_default()
}

/// Declared theme names vs the registry (MERGE.md C2), before render.
/// Name half only — subtheme tokens are CSS fodder. Themeless rows skip
/// (site default already checked in `load_all`).
fn check_theme_names(cfg: &Config, db: &SiteDb, themes: &theme::Themes) -> Result<()> {
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

/// Render every routable URL into memory. Writes nothing to the output; the
/// only disk it touches is the content-addressed `_cache/` (thumbnails, §6b).
pub fn render_site(cfg: &Config, db: &mut SiteDb) -> Result<(SiteOutput, Stats)> {
    let mut out_map: SiteOutput = BTreeMap::new();

    let icon = site_icon(cfg, db);
    let site = Site {
        url: &cfg.site.url,
        title: &cfg.site.title,
        author: &cfg.site.author,
        email: cfg.site.email.as_deref(),
        icon: &icon,
    };
    let profile = cfg.profile.as_deref();
    // `[html.head.meta]` (§4e), compiled once against both surfaces.
    let metas = render::compile_metas(cfg, &db.declared)?;
    let mut stats = Stats::default();

    let root = cfg.root();
    let theme_dir = root.join("themes/default");

    let thumbs = thumbs_pass(cfg, db, &root, &mut out_map, &mut stats)?;

    // An image field's published URL (§5e image parts): the thumbnail's when
    // the pass generated one, else the original under baseurl. This is the
    // presentation `fill_from_fields` delegates so it need not know either.
    let resolve_asset = |src: &str| -> String {
        crate::thumbs::default_of(&thumbs, src)
            .map(|t| t.url.clone())
            .unwrap_or_else(|| asset_url(&cfg.site.baseurl, src))
    };

    // ---- themes: every directory under themes/, loaded once (§5e). All
    // theme errors — malformed fragment, unknown slot, arity violation —
    // surface here, before anything renders. Theme is chosen per ROW (§5a).
    // §5e: the part vocabulary this build runs against — the engine's kinds
    // plus whatever `[[parts]]` the site declares. Fragments are checked
    // against it, so a theme can place a part the site invented.
    let schemas = parts::Schemas::engine_only();
    let themes = theme::Themes::load_all(
        &root.join("themes"),
        &root,
        &schemas,
        cfg.site.theme.as_deref(),
    )
    .context("loading themes")?;
    check_theme_names(cfg, db, &themes)?;
    // C4b: a `.slots/` file whose stem names no slot any loaded theme places
    // fills nothing, silently. Said here rather than in the source loader
    // because the knowledge is here — the slot names come from the themes,
    // which only exist once `load_all` has run. A warning, not an error;
    // `slots::unknown_stems` carries the reasoning. `serve` rebuilds the
    // world through this function on every change, so a fixed name stops
    // being reported on the next save (C3's convention, one crate over).
    {
        let locales = cfg.locales();
        for w in crate::slots::unknown_stems(themes.fills(), &themes.identity_slots(), &locales) {
            eprintln!("grackle: {w}");
            db.warnings.push(w);
        }
    }

    // §6a row/view links: the resolution space, once per build.
    let linkspace = crate::links::LinkSpace::new(cfg, db, &root);
    let bodies = render_bodies(cfg, db, &thumbs, &linkspace)?;
    let page_bodies = render_page_bodies(cfg, db, &site, &themes, &thumbs, &linkspace)?;

    // ---- the link graph (q38): scan every rendered body once — posts and
    // pages alike — and invert. Backlinks are one more relations axis; the
    // scan reads the same bytes that ship, so link and index cannot desync.
    let (backlinks, links_to) = backlinks_map(db, &bodies, &page_bodies, &cfg.site.url);

    // ---- related posts (§6b): cache-only load — fresh vectors where the
    // cache has them, STALE ones where a post's text changed (it keeps its
    // old embedding until reprocessed), None for never-seen posts. Whatever
    // is pending goes back to the caller via Stats: `build` embeds it
    // before rendering (published output is always fresh), `serve` embeds
    // on a background thread and re-renders on completion. Ranking policy
    // ([related]: min score, year penalty/cap) is config.
    let loaded = match crate::embed::load(db, &root.join("_cache/embeddings")) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("grackle: embeddings unavailable, skipping related posts: {e:#}");
            crate::embed::Loaded {
                vectors: Vec::new(),
                pending: Vec::new(),
            }
        }
    };
    stats.embed_pending = loaded.pending;

    // §6g: the relation engine — declared neighbour queries evaluated per row,
    // with the embedding vectors and the link graph in hand. Replaces the
    // hardcoded similar/adjacency/linked-from axes; `[related]`'s knobs are
    // now grack.com's `related` rank expression. Evaluated for every rendered
    // row up front into an owned map, so the engine's borrow of `db` ends
    // before the render passes need `&mut db` again.
    let rel_groups: HashMap<String, Vec<crate::relate::Group>> = {
        let relate = crate::relate::Engine::new(cfg, db, &loaded.vectors, &links_to, &backlinks);
        db.rows
            .iter()
            .filter(|r| r.rendered)
            .map(|r| (r.url.clone(), relate.groups_for(r)))
            .collect()
    };

    // ---- posts: document parts -> theme fragments -> shell
    //
    // Driven by the ROUTE table, like the `RouteKind::Page` arm below. It used
    // to iterate `post_ix` and key its output by `p.url`, which is the same
    // thing only while a row has exactly one route — and "a row has one URL"
    // was an assumption the design never made and six maps in here relied on.
    // Iterating routes means the URL being rendered comes from the route, and a
    // second route onto one row (q53) renders twice rather than colliding in
    // `out_map`.
    //
    // `bodies` holds the in-memory bodies the posts loader produced, keyed by
    // ROW rather than by URL for the same reason.
    //
    // `kind == Post` survives I13 here: it is scope membership on the output
    // side, and the route pool has no other column for that (see `RouteKind`'s
    // own doc for the census).
    let post_routes: Vec<&Route> = db
        .routes
        .iter()
        .filter(|r| r.kind == RouteKind::Post)
        .collect();
    let rendered: Vec<(String, String)> = post_routes
        .par_iter()
        .filter_map(|r| r.row.as_ref().and_then(|k| db.rows.get(k)).map(|p| (*r, p)))
        .map(|(r, p)| -> Result<(String, String)> {
            let url = r.url.as_str();
            let mut head = render::head_for_post(p, &site);
            // The head describes the DOCUMENT, and a document's address is its
            // canonical URL — `p.url`, which is exactly what the canonical axis
            // member is published at (an alternate is templated, the canonical
            // one is not). So an alternate's `rel="canonical"` and `og:url`
            // name the canonical form rather than themselves, which is the
            // whole difference between an alternative form and a duplicate
            // page. For a row on no axis this is the route's own URL anyway.
            head.meta = render::eval_metas(&metas, p, &site, &head.title, &p.url);
            let trail = crate::trails::post_trail(cfg, db, p);
            let whole = bodies[&p.key].whole.as_str();
            // §6e: toc rows carry outline from the same rendered bytes (h2–h3).
            let outline = if p.flag("toc") {
                let tree = crate::outline::heading_tree(&bodies[&p.key].headings(), 2, 3);
                crate::outline::to_parts(&tree, &p.url)
            } else {
                Vec::new()
            };
            let translations = locale_twins(db, p);
            let mut head = head;
            head.alternates = locale_alternates(&cfg.site.url, &p.locale, &p.url, &translations);
            head.alternates
                .extend(axis_alternates(db, &cfg.site.url, r));
            let groups =
                parts::relation_groups(rel_groups.get(&p.url).cloned().unwrap_or_default());
            let doc = parts::document(cfg, p, whole, trail, groups, outline);
            let dir = p.path.parent().unwrap_or(&root);
            let (theme_name, subtheme) = resolve_theme(&themes, r, p.theme.as_deref());
            let row_thm = themes.get(theme_name)?;
            let html = chain::document_page(
                chain::Page {
                    theme: row_thm,
                    head_html: render::head_html(
                        &head,
                        &theme::css_url(&cfg.site.baseurl, theme_name),
                    ),
                    site_title: &cfg.site.title,
                    source_dir: dir,
                    locale: &p.locale,
                    resolve_link: &fill_link_resolver(cfg, &linkspace, &p.locale),
                    subtheme: subtheme.as_deref(),
                    profile,
                    axis: &r.axis,
                    axes: axes_part(cfg, db, r),
                },
                Some(p),
                doc,
                whole,
                &resolve_asset,
            )?;
            Ok((url.to_string(), html))
        })
        .collect::<Result<Vec<_>>>()?;
    for (url, html) in rendered {
        out_map.insert(url, html.into_bytes());
        stats.posts += 1;
    }

    // ---- one walk of the route table for aggregates (§9b).
    //
    // Layouted non-landing routes go through the listing pass; `layout` /
    // `variant` only pick the member face the theme must ship.
    {
        let ctx = crate::passes::Ctx {
            metas: &metas,
            cfg,
            db,
            site: &site,
            themes: &themes,
            thumbs: &thumbs,
            bodies: &bodies,
            page_bodies: &page_bodies,
            linkspace: &linkspace,
            backlinks: &backlinks,
            root: root.clone(),
            profile,
            objects: db.object_ix.iter().collect(),
        };
        crate::passes::run(&ctx, &crate::passes::all(), &mut out_map, &mut stats)?;
    }

    // ---- landings (q45 mode B): routes whose view claims a content row.
    //
    // The row is the whole body and owns the arrangement; `{% view <owner> %}`
    // substitutes THIS route's slice — page 2 renders page 2's rows, /fr/ the
    // French partition — built by base kind exactly as the bare passes build
    // it, minus title and crumbs (those are the row's to place). The row
    // keeps everything rows have: front matter, its rule-derived theme, its
    // directory (slot fills resolve nearest-wins from there), suffix
    // localization with default-locale fallback.
    let mut section_trees: HashMap<PathBuf, Vec<crate::outline::Node>> = HashMap::new();
    for r in &db.routes {
        let Some(view) = &r.view else { continue };
        let Some(v) = cfg.views.get(view) else {
            continue;
        };
        // Per-route content (templated `content`, or a templated
        // `default_content` this route accepted) beats the view-level literal
        // one, which is what lets one grouped view give each route its own
        // words (§5c). A view with neither has nothing to embed.
        let Some(content) = r.content.as_deref().or(v.content.as_deref()) else {
            continue;
        };
        // (A `kind != View` guard stood here and was DELETED at I13, not
        // respelled: the `let Some(view)` four lines up already asked it —
        // "is this a view route" is the `view` column being non-empty.)
        let loc = r.locale.as_deref().unwrap_or(&cfg.i18n.default);

        // The claimed row, in the route's locale — else the default's
        // prose (the same fallback slot fills use).
        let sibs = db.by_logical.get(content).cloned().unwrap_or_default();
        let row = sibs
            .iter()
            .filter_map(|k| db.rows.get(k))
            .find(|p| p.locale == loc)
            .or_else(|| {
                sibs.iter()
                    .filter_map(|k| db.rows.get(k))
                    .find(|p| p.locale == cfg.i18n.default)
            });
        let Some(row) = row else { continue }; // existence-checked at load
        let src = &row.path;

        if view_base_collection(cfg, view).is_none() {
            continue;
        }
        let items = member_previews(
            cfg,
            db,
            view,
            &r.members,
            &thumbs,
            &bodies,
            &page_bodies,
            |k| db.object_ix.iter().any(|o| o == k),
        );
        let (theme_name, subtheme) =
            resolve_view_theme(&themes, r, v.theme.as_deref(), || {
                themes.resolve(row.theme.as_deref())
            });
        let row_thm = themes.get(theme_name)?;
        let layout = v.layout.as_deref().with_context(|| {
            format!("view {view}: landing embed needs a layout (member face)")
        })?;
        let mut embed_html = chain::member_faces(
            &row_thm.fragments,
            layout,
            v.variant.as_deref(),
            items,
        )
        .with_context(|| format!("view {view}"))?;
        if let Some(p) = pagination_parts(db, view, v, r)? {
            embed_html.push_str(&row_thm.fragments.render(&p));
        }

        // Must-place (q45): the claimed row owns the arrangement — a body
        // that never places the owner's embed strands the view's rows.
        // (A `default_content` claim only exists if the row already placed the
        // embed — see `resolve_default_content` — so this stays exact.)
        let text =
            std::fs::read_to_string(src).with_context(|| format!("reading {}", src.display()))?;
        let (_, body) = split_front_matter(&text);
        let tag = format!("{{% view {view} %}}");
        if !body.contains(&tag) {
            bail!(
                "view {view}: content {} never places {tag} — the claimed row \
                 owns the arrangement, and without the embed the view's rows \
                 are unreachable",
                row.rel.display()
            );
        }

        // Expand with a sentinel, render markdown, then substitute the
        // slice — so the embedded HTML never meets the markdown parser
        // (a blank line inside it would split the HTML block).
        const SENTINEL: &str = "<!--grackle:landing-embed-->";
        let cx = tags::Ctx {
            includes: Some(cfg.root().join("_includes")),
            site: Some(&site),
            thumbs: Some(&thumbs),
            theme: Some(row_thm),
            widgets: Some(&cfg.widgets),
            links: Some(&linkspace),
            embed: Some((view.as_str(), SENTINEL)),
            ..tags::Ctx::new(db, &cfg.site.baseurl, src.display().to_string())
        };
        let expanded = tags::expand(body, &cx)?;
        // Body links resolve at the ROUTE's locale (the slot-fill precedent:
        // prose follows its reader), from the row's dir. No route exemption
        // is needed on this path, unlike the bare page path above: the embed
        // is still a SENTINEL here, so engine-derived URLs are not in the
        // document yet and everything the resolver meets is authored.
        let dir = row.rel.parent().map(Path::to_path_buf).unwrap_or_default();
        let rel = row.rel.to_string_lossy().to_string();
        let resolve = |form: crate::links::Cite, href: &str| {
            crate::links::resolve(cfg, &linkspace, &dir, &r.url, loc, &rel, form, href)
        };
        let (frag, _) = crate::markdown::render_source(
            &expanded,
            src.extension().is_some_and(|e| e == "md"),
            &resolve,
        )?;
        let frag = frag.replace(SENTINEL, &embed_html);

        // Title: the row's front matter beats the view's declaration
        // (explicit beats derived, per row) — the trail's inert tail
        // follows it.
        let (vtitle, mut trail) = crate::trails::listing_title_and_trail(cfg, db, view, v, r)?;
        let title = row.title.clone().unwrap_or(vtitle);
        if let Some(last) = trail.last_mut() {
            if last.1.is_none() {
                last.0 = title.clone();
            }
        }

        // The landing's locale switcher and any axis are the `axes` slot now,
        // computed per route by `axes_part` — a landing per locale IS the
        // translation set (a fallback landing is still the French landing).
        let section = section_parts(
            db,
            &mut section_trees,
            &row.rel,
            &r.url,
            &cfg.i18n.default,
        );
        let groups = parts::relation_groups(rel_groups.get(&r.url).cloned().unwrap_or_default());
        let doc = parts::document_tree(
            cfg,
            loc,
            &crate::trails::home_url(cfg, db, loc),
            &title,
            &r.url,
            &crate::trails::ancestors(cfg, db, &r.url),
            section,
            Vec::new(),
            None,
            groups,
            &frag,
        );
        let head = render::head_for(&title, &r.url, &site, &metas, r);
        let dir = src.parent().unwrap_or(&root);
        let html = chain::document_page(
            chain::Page {
                theme: row_thm,
                head_html: render::head_html(
                    &head,
                    &theme::css_url(&cfg.site.baseurl, theme_name),
                ),
                site_title: &cfg.site.title,
                source_dir: dir,
                locale: loc,
                resolve_link: &fill_link_resolver(cfg, &linkspace, loc),
                subtheme: subtheme.as_deref(),
                profile,
                axis: &r.axis,
                axes: axes_part(cfg, db, r),
            },
            Some(row),
            doc,
            &frag,
            &resolve_asset,
        )?;
        out_map.insert(r.url.clone(), html.into_bytes());
        stats.listings += 1;
    }

    // ---- feed: the atom.xml serialization (the `feed` view's template).
    //
    // A serialization, not a themed page — it bypasses the shell entirely (§5e:
    // "feed bypasses themes; serializations have no look"). The route already
    // carries its members (the 20 newest published, newest-first); we render
    // each body and apply the feed's content transforms.
    let updated = chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S+00:00")
        .to_string();
    for r in &db.routes {
        let Some(view) = &r.view else { continue };
        let Some(v) = cfg.views.get(view) else {
            continue;
        };
        // The atom SHELL: the same rows, a different outermost wrapper —
        // declared, not inferred from a template filename (q44).
        if v.shell.as_deref() != Some("atom") {
            continue;
        }
        // `members` indexes one row store whatever the view's base table, so
        // the kind does not enter into it. Finding the row's HTML does: posts
        // hold their body, tree rows are re-read, and the two maps below
        // answer that. A dated tree collection can have a feed.
        let entries: Vec<(&crate::db::Row, &str)> = r
            .members
            .iter()
            .filter_map(|k| db.rows.get(k))
            .map(|p| (p, row_body_html(p, &bodies, &page_bodies).unwrap_or("")))
            .collect();
        let xml = render::feed(&site, &r.url, &updated, &entries);
        out_map.insert(r.url.clone(), xml.into_bytes());
        stats.serialized += 1;
    }

    // ---- sitemap: a fold with no `from` serializes the finished route set.
    //
    // The fold (§5) counted its matches at load; here we read them back. `lastmod` is emitted only for posts, from the
    // content date. jekyll-sitemap also stamps static files with their file
    // *mtime* — but that is checkout-time noise (every clone differs) and works
    // against the indexing goal this whole project exists for, so it is
    // deliberately dropped — the URL *set* is unaffected. (DESIGN §4a is the
    // related draft/hidden concern.)
    for fold in &db.routes {
        let Some(view) = &fold.view else { continue };
        let Some(v) = cfg.views.get(view) else {
            continue;
        };
        // The sitemap SHELL, likewise declared.
        if v.shell.as_deref() != Some("sitemap") {
            continue;
        }
        // Resolved at load like every other view's, rather than re-derived
        // from the filter's source text here.
        let entries: Vec<(String, Option<String>)> = fold
            .route_members
            .iter()
            .filter_map(|k| db.routes.get(k))
            .map(|r| {
                let loc = format!("{}{}", site.url, r.url);
                // `lastmod` follows the DATE, not the table — a dated tree
                // row gets one too (q51).
                let lastmod = db
                    .row_by_url(&r.url)
                    .and_then(|p| p.date)
                    .map(render::xmlschema);
                (loc, lastmod)
            })
            .collect();
        let xml = render::sitemap(&entries);
        out_map.insert(fold.url.clone(), xml.into_bytes());
        stats.serialized += 1;
    }

    // ---- script shells (§5g, the pun intended): registered serializations.
    //
    // The experimental bench: a `[shells.name] command = "…"` entry plus
    // `shell = "name"` on a view pipes the view's member rows as JSON into
    // the command's stdin, and whatever bytes it prints land at the view's
    // route verbatim — PDF, PostScript, whatever. The JSON schema is TEMP
    // (stamped "grackle-shell/0"); it gets versioned the day anything
    // beyond an experiment depends on it. A shell that earns keeping gets
    // promoted to a built-in.
    for r in &db.routes {
        let Some(view) = &r.view else { continue };
        let Some(v) = cfg.views.get(view) else {
            continue;
        };
        let Some(shell) = v.shell.as_deref() else {
            continue;
        };
        let Some(def) = cfg.shells.get(shell) else {
            continue;
        };
        let rows: Vec<serde_json::Value> = r
            .members
            .iter()
            .filter_map(|k| db.rows.get(k))
            .map(|p| {
                serde_json::json!({
                    "url": p.url,
                    "title": p.title,
                    "date": p.date.map(crate::db::iso_date),
                    "date_pretty": p.date.map(crate::db::pretty_date),
                    "tags": p.tags,
                    "html": row_body_html(p, &bodies, &page_bodies).unwrap_or(""),
                })
            })
            .collect();
        let payload = serde_json::json!({
            "schema": "grackle-shell/0",
            "shell": shell,
            "view": view,
            "route": r.url,
            "site": { "url": site.url, "title": site.title, "author": site.author },
            "rows": rows,
        });
        let bytes = run_script_shell(&root, &def.command, &payload)
            .with_context(|| format!("view {view}: script shell {shell:?} ({})", def.command))?;
        out_map.insert(r.url.clone(), bytes);
        stats.serialized += 1;
    }

    // ---- tree: rendered pages + static passthrough + objects
    //
    // Section trees (§6e) derive once per `.section` root and are re-shaped
    // per page — the tree is shared with the landing pass, only `current`
    // moves.
    //
    // **The dispatch that survives `kind`** (IO.md I13). Half of it is
    // respellable in facts and half is not, and taking the half would cost
    // more than it buys:
    //
    // - `Static | Object` vs `Page` IS the rendering law's output. Measured on
    //   all six corpus trees: every `Static` and every `Object` route's row is
    //   `rendered false`, every `Page` route's row is `rendered true`. So this
    //   `match` could ask `p.rendered` instead of naming three variants.
    // - `Post` vs `Page` is NOT expressible. Posts render above, from their
    //   own body store; "this row is in a posts scope" is a fact about the
    //   CONFIG, and a row carries the scope's name and not its role (I9's
    //   ruling, one store over). So the `_ => {}` arm would have to stay a
    //   `kind` test whatever happens to the other two.
    //
    // A `match` that dispatches on which pass owns an output is what this enum
    // IS; respelling two of its five arms and leaving a `kind == Post` guard
    // above them makes one construct into three and reads worse. Declined,
    // with the measurement recorded rather than the option forgotten.
    for r in &db.routes {
        match r.kind {
            RouteKind::Static | RouteKind::Object => {
                let Some(src) = &r.source else { continue };
                let bytes =
                    std::fs::read(src).with_context(|| format!("reading {}", src.display()))?;
                out_map.insert(r.url.clone(), bytes);
                stats.copied += 1;
            }
            RouteKind::Page => {
                let Some(src) = &r.source else { continue };
                let row = r.row.as_ref().and_then(|k| db.rows.get(k));
                let title = row.and_then(|p| p.title.clone()).unwrap_or_default();

                // Bodies were rendered in the prepass (so the link graph
                // could scan them); scss and unknown-construct pages were
                // recorded there too.
                let Some(pb) = page_bodies.get(&r.url) else {
                    continue;
                };
                if pb.skipped {
                    stats.skipped.push(r.url.clone());
                    continue;
                }
                let frag = &pb.frag;
                // §6e heading axis for `toc:` pages, from the prepass Doc.
                let outline = match (&pb.doc, row.is_some_and(|p| p.flag("toc"))) {
                    (Some(d), true) => {
                        let tree = crate::outline::heading_tree(&d.headings(), 2, 3);
                        crate::outline::to_parts(&tree, &r.url)
                    }
                    _ => Vec::new(),
                };

                let section = row
                    .map(|p| {
                        section_parts(db, &mut section_trees, &p.rel, &r.url, &cfg.i18n.default)
                    })
                    .unwrap_or_default();

                // Hero (q23): image-typed field, thumbnailed, with dimensions.
                let hero = row.and_then(|p| p.hero_source()).map(|s| {
                    let t = crate::thumbs::default_of(&thumbs, s);
                    let full = asset_url(&cfg.site.baseurl, s);
                    parts::preview(parts::Preview {
                        title: Some(title.clone()),
                        url: Some(full.clone()),
                        src: Some(t.map(|t| t.url.clone()).unwrap_or(full)),
                        dims: t.and_then(|t| t.dims),
                        ..Default::default()
                    })
                });

                // Theme per row (§5a); axis theme beats the row's (q53).
                let (theme_name, subtheme) =
                    resolve_theme(&themes, r, row.and_then(|p| p.theme.as_deref()));
                let row_thm = themes.get(theme_name)?;
                let row_css = theme::css_url(&cfg.site.baseurl, theme_name);
                // Metas read the ROW when present; sourceless routes use the route.
                let head = match row {
                    Some(p) => render::head_for(&title, &p.url, &site, &metas, p),
                    None => render::head_for(&title, &r.url, &site, &metas, r),
                };
                // IO.md §4: the output picks its map shell. `raw` is the
                // transparent one — the body IS the output, so an imported
                // document can carry front matter (title, tags, hidden)
                // without being nested inside a second `<html>`.
                // q53: an axis member over `shell` is the md twin's shape — the
                // same row serialized two ways, at two URLs. The member's value
                // beats the row's own for the same reason a member's theme
                // does: the member IS the alternative form.
                //
                let shell = axis_field(r, "shell").or(row.and_then(|p| p.shell.as_deref()));
                if shell == Some("raw") {
                    out_map.insert(r.url.clone(), frag.clone().into_bytes());
                    stats.pages += 1;
                    continue;
                }
                let tier = match shell {
                    Some("light_html") => Theme::Light,
                    _ => Theme::Default,
                };
                let row_locale = row
                    .map(|p| p.locale.as_str())
                    .unwrap_or(cfg.i18n.default.as_str());
                let html = match tier {
                    Theme::Light => chain::light_page(&head, row_locale, profile, &r.axis, frag),
                    Theme::Default => {
                        let groups = parts::relation_groups(
                            rel_groups.get(&r.url).cloned().unwrap_or_default(),
                        );
                        let translations = row.map(|p| locale_twins(db, p)).unwrap_or_default();
                        let mut head = head;
                        head.alternates =
                            locale_alternates(&cfg.site.url, row_locale, &r.url, &translations);
                        head.alternates
                            .extend(axis_alternates(db, &cfg.site.url, r));
                        let doc = parts::document_tree(
                            cfg,
                            row_locale,
                            &crate::trails::home_url(cfg, db, row_locale),
                            &title,
                            &r.url,
                            &crate::trails::ancestors(cfg, db, &r.url),
                            section,
                            outline,
                            hero,
                            groups,
                            frag,
                        );
                        let dir = src.parent().unwrap_or(&root);
                        chain::document_page(
                            chain::Page {
                                theme: row_thm,
                                head_html: render::head_html(&head, &row_css),
                                site_title: &cfg.site.title,
                                source_dir: dir,
                                locale: row_locale,
                                resolve_link: &fill_link_resolver(cfg, &linkspace, row_locale),
                                subtheme: subtheme.as_deref(),
                                profile,
                                axis: &r.axis,
                                axes: axes_part(cfg, db, r),
                            },
                            row,
                            doc,
                            frag,
                            &resolve_asset,
                        )?
                    }
                };
                out_map.insert(r.url.clone(), html.into_bytes());
                stats.pages += 1;
            }
            _ => {}
        }
    }

    search_pass(cfg, db, &bodies, &page_bodies, &mut out_map, &mut stats)?;

    // C5(d): a link query key that looks like an axis selector but names no
    // declared axis. Drained here, after every body has been resolved and once
    // `bodies` has released its borrow of `db` — the resolver runs inside two
    // parallel render passes and has nowhere to write until now. Same channel
    // as C3/C4: a `grackle: ` line on stderr, and `db.warnings` for the tests.
    for w in linkspace.take_warnings() {
        eprintln!("grackle: {w}");
        db.warnings.push(w);
    }
    let overlay = site_overlay(&root, &mut stats);
    // Each theme's sheet carries that theme's own `root.html` head styles
    // (I5) — the head fence's whole purpose, and the reason no page needs an
    // inline `<style>`. `get(None)` is the `default` directory or the base,
    // matching theme::css_url's `/css/main.css`.
    css_pass(
        &theme_dir,
        themes.get(None)?.head_style(),
        "/css/main.css",
        overlay.as_deref(),
        &mut out_map,
        &mut stats,
    )?;
    for name in themes.names().filter(|n| *n != "default") {
        css_pass(
            &root.join("themes").join(name),
            themes.get(Some(name))?.head_style(),
            &format!("/css/{name}.css"),
            overlay.as_deref(),
            &mut out_map,
            &mut stats,
        )?;
    }

    // One scan of the finished output, read twice: the pull model's frontier,
    // and IO.md §2's citation edges. `materialize_referenced` extends it with
    // whatever it publishes.
    let mut cited = citation_map(&out_map, &cfg.site.url);
    stats.on_demand = materialize_referenced(db, &mut out_map, &cfg.site.url, &mut cited)?;

    // IO.md §2: the half of `output.inputs` that only content can answer.
    join_citations(db, &cited);

    // IO.md §4a: the renditions this build materialized, entered as outputs
    // with their parameters and their edges. After `join_citations` because it
    // reads the same one scan of the finished output, and because a citing
    // route has to exist before it can gain an edge.
    join_renditions(db, &thumbs, &cited);

    // The §6g splice markers have done their two jobs — fencing the citation
    // scan (backlinks) while on-demand publishing above read past them — so
    // strip them before the bytes ship. Last pass, after every scanner.
    strip_view_markers(&mut out_map);
    Ok((out_map, stats))
}

/// The citation half of `Route.inputs` (IO.md §2), added once the bytes exist.
///
/// **Facts at planning; content at materialization** — and `inputs` is the one
/// join field that straddles the line. `load::join_arrangement` filled every
/// edge planning knows (the row a route renders, a landing's claimed body, a
/// view's members, the rows behind a fold's selected routes); the rest of the
/// row-level closure is *cited* rows, and a citation is a fact about finished
/// output. An `{% image %}` expands to markup no body contains, so there is no
/// earlier moment this could be honest.
///
/// The same scanner on-demand publishing uses, and deliberately the unfenced
/// one (`cited_urls`, not `cited_urls_cited`): a spliced arrangement's links
/// are not citations for the backlink graph's purpose, but an image a listing
/// arranged is still an input to the bytes. What §6g's fence keeps out of
/// "linked from" it must not keep out of "what would a rebuild need".
///
/// Cited URLs that name no row are skipped rather than recorded: an
/// output→output edge is `route_members`, and an external link is not an edge
/// at all.
fn join_citations(db: &mut SiteDb, cited: &[(String, Vec<String>)]) {
    let mut found: Vec<(grackle_db::Key, Vec<grackle_db::Key>)> = Vec::new();
    for (url, urls) in cited {
        let rows: Vec<grackle_db::Key> =
            urls.iter().flat_map(|u| resolve_citation(db, u)).collect();
        if rows.is_empty() {
            continue;
        }
        found.push((grackle_db::Key::new(url), rows));
    }
    for (route, rows) in found {
        let Some(r) = db.routes.get_mut(&route) else {
            continue;
        };
        r.inputs.extend(rows);
        r.inputs.sort();
        r.inputs.dedup();
    }
}

/// The inputs one cited URL names (IO.md §4a, I11) — the address resolution
/// both halves of the citation seam read, so the pull and the join cannot
/// disagree about what an address means.
///
/// Two slots, two indexes. `by_url` answers a CANONICAL address and answers it
/// uniquely. `by_strong` answers a hash address and answers it with a LIST,
/// because the address is a pure function of the bytes and inputs holding one
/// byte string share it. All of them are edges: over-approximating here costs
/// a rebuild an extra output to reconsider, while dropping one is the stale
/// page the graph exists to prevent.
///
/// A URL in neither is not an edge at all — an external link, or an output
/// (whose edge is `route_members`).
fn resolve_citation(db: &SiteDb, url: &str) -> Vec<grackle_db::Key> {
    if let Some(k) = db.by_url.get(url) {
        return vec![k.clone()];
    }
    db.by_strong.get(url).cloned().unwrap_or_default()
}

/// Every internal citation of every finished output, by the URL it was
/// published at.
///
/// Each document is scanned against its OWN url, because a citation is usually
/// relative: `code/legacy/romtool/index.html` says `<img src="screen1.png">`,
/// which §6a records as how that content has always been organised. Binary
/// entries fail the UTF-8 gate and cite nothing.
///
/// One scan, two consumers (on-demand publishing and IO.md §2's citation
/// edges) — the seam that keeps the join's closure from costing a second pass
/// over the whole site.
fn citation_map(out: &SiteOutput, site_url: &str) -> Vec<(String, Vec<String>)> {
    out.iter()
        .filter_map(|(u, b)| std::str::from_utf8(b).ok().map(|t| (u, t)))
        .filter_map(|(u, t)| {
            let c = cited_urls(t, u, site_url);
            (!c.is_empty()).then(|| (u.clone(), c))
        })
        .collect()
}

/// Remove the `{% view %}` fence comments from the finished output (§6g). Only
/// touches the few pages that carry them; binary entries fail the UTF-8 gate
/// and are skipped.
fn strip_view_markers(out: &mut SiteOutput) {
    for bytes in out.values_mut() {
        let Ok(s) = std::str::from_utf8(bytes) else {
            continue;
        };
        if !s.contains("<!--grackle:view-->") {
            continue;
        }
        *bytes = s
            .replace("<!--grackle:view-->", "")
            .replace("<!--/grackle:view-->", "")
            .into_bytes();
    }
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
fn thumbs_pass(
    cfg: &Config,
    db: &SiteDb,
    root: &Path,
    out_map: &mut SiteOutput,
    stats: &mut Stats,
) -> Result<crate::thumbs::Renditions> {
    let mut asks: Vec<crate::thumbs::Ask> = Vec::new();
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
                .map(|s| (s.clone(), Rendition::THUMB)),
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
                    let (_, body) = split_front_matter(&text);
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
                        asks.push((o.rel.to_string_lossy().to_string(), Rendition::THUMB));
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

/// **Renditions as outputs** (IO.md §4a, I12): the artifacts `thumbs_pass`
/// published, entered into the outputs database with the edges and the
/// parameters that say what they are.
///
/// Two halves, and the pair is the whole model:
///
/// 1. **A rendition is an output of its input.** One `Route` per distinct
///    address, carrying `inputs` — the rows whose bytes fed the transform —
///    and `rendition`, the parameters it was made with. That pair is the
///    reproduction recipe: read those bytes, run `thumbs::render` with those
///    parameters, get these bytes back. The edge runs **input → output**,
///    because the transform reads the INPUT's bytes and never the original
///    output's; see `graph.rs` for what that answers.
/// 2. **The citing edge names it.** An output whose finished bytes embed a
///    rendition address gains a FACTS edge to it (`route_members`) — it read
///    the rendition's *url*, which the hashing law makes knowable at planning,
///    and not its content — plus the CONTENT edges to the rows behind it,
///    because those bytes are what its address is a function of.
///
/// The second half's content edges are load-bearing where an affordance shows
/// a rendition and links nothing else: a LISTING with a hero picture cites only
/// `/static/{hash}`, so without this its `inputs` would lose the image and
/// editing that image would ship a stale listing. `{% image %}` happens to link
/// the original beside its thumbnail, which is why the same edge arrives twice
/// on a post page and exactly once here.
///
/// Runs at build, like `materialize_referenced` and for the same reason: a
/// rendition exists because a citation asked for it, and citations live in
/// finished bytes. So the CLI's load-only surfaces (`explain`, `query pull`)
/// do not see these outputs, which is what they already do for on-demand ones.
fn join_renditions(
    db: &mut SiteDb,
    thumbs: &crate::thumbs::Renditions,
    cited: &[(String, Vec<String>)],
) {
    if thumbs.is_empty() {
        return;
    }
    // Rung 0 reaches this seam too (IO.md I10's law, stated at load.rs's
    // `force_route_fields`): minting an output is the graph event, so a seam
    // that mints applies the profile's forced fields. I11 added a shape to an
    // existing seam; this is the third seam, and the law is why it is one line
    // rather than a rediscovery.
    let forced: Vec<(String, grackle_db::filter::Value)> = db
        .forced_fields
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    // One output per address; its inputs are every source whose bytes landed
    // there. The union is over ROWS, so a source no rule admitted contributes
    // an artifact with no edge rather than a dangling key.
    let mut by_address: BTreeMap<String, (grackle_model::Rendition, Vec<grackle_db::Key>)> =
        BTreeMap::new();
    for ((src, ask), t) in thumbs {
        let e = by_address
            .entry(t.address.clone())
            .or_insert_with(|| (*ask, Vec::new()));
        let key = grackle_db::Key::new(src);
        if db.rows.get(&key).is_some() {
            e.1.push(key);
        }
    }
    for (address, (rendition, inputs)) in &mut by_address {
        inputs.sort();
        inputs.dedup();
        if db.routes.get(&grackle_db::Key::new(address)).is_some() {
            continue;
        }
        let mut route = Route {
            // A rendition's canonical address IS its strong address: no rule
            // minted it, the content store did (I11's reading, one transform
            // along).
            strong_url: Some(address.clone()),
            inputs: inputs.clone(),
            rendition: Some(*rendition),
            ..Route::new(address.clone(), RouteKind::Object)
        };
        for (name, value) in &forced {
            route.fields.insert(name.clone(), value.clone());
        }
        db.routes.push(route);
    }

    // The citing edges. A citation writes the baseurl-bearing URL while an
    // output's key does not carry one, so both spellings are indexed — the
    // same seam `Row.url` has, answered here rather than left.
    let mut addr_of: HashMap<&str, &String> = HashMap::new();
    for t in thumbs.values() {
        addr_of.insert(t.address.as_str(), &t.address);
        addr_of.insert(t.url.as_str(), &t.address);
    }
    let mut found: Vec<(grackle_db::Key, Vec<grackle_db::Key>, Vec<grackle_db::Key>)> = Vec::new();
    for (url, urls) in cited {
        let mut facts: Vec<grackle_db::Key> = Vec::new();
        let mut content: Vec<grackle_db::Key> = Vec::new();
        for u in urls {
            let Some(address) = addr_of.get(u.as_str()) else {
                continue;
            };
            let key = grackle_db::Key::new(address);
            if let Some((_, inputs)) = by_address.get(*address) {
                content.extend(inputs.iter().cloned());
            }
            facts.push(key);
        }
        if facts.is_empty() {
            continue;
        }
        found.push((grackle_db::Key::new(url), facts, content));
    }
    for (route, facts, content) in found {
        let Some(r) = db.routes.get_mut(&route) else {
            continue;
        };
        r.route_members.extend(facts);
        r.route_members.sort();
        r.route_members.dedup();
        r.inputs.extend(content);
        r.inputs.sort();
        r.inputs.dedup();
    }
}

/// ONE render per post (§6d). Expand + parse once; the same parse yields the
/// whole document (posts, feed) and the block sequence each listing view
/// projects its summaries from.
/// The Doc is kept whole because truncation is VIEW policy (`summary = {
/// max_blocks, max_chars }`), not a property of the body.
fn render_bodies<'a>(
    cfg: &Config,
    db: &'a SiteDb,
    thumbs: &crate::thumbs::Renditions,
    linkspace: &crate::links::LinkSpace,
) -> Result<HashMap<&'a grackle_db::Key, Doc>> {
    let root = cfg.root();
    // Posts only: these rows hold their body in memory. Tree rows are
    // re-read at render time (§2), which `render_page_bodies` does — the
    // loader asymmetry that outlives the row-type merge.
    //
    // Keyed by ROW, not by URL: a body is a property of the row, and keying it
    // by URL quietly asserted that a row has one.
    db.post_ix
        .par_iter()
        .filter_map(|k| db.rows.get(k))
        .map(|p| -> Result<(&grackle_db::Key, Doc)> {
            let cx = tags::Ctx {
                thumbs: Some(thumbs),
                widgets: Some(&cfg.widgets),
                links: Some(linkspace),
                ..tags::Ctx::new(db, &cfg.site.baseurl, p.path.display().to_string())
            };
            let body = crate::store::read_body(&p.path)?;
            let expanded = tags::expand(&body, &cx)?;
            // §6a row/view links: destinations resolve against the
            // database, relative to this post's source directory.
            let dir = p
                .path
                .strip_prefix(&root)
                .ok()
                .and_then(|r| r.parent().map(Path::to_path_buf))
                .unwrap_or_default();
            let doc = crate::markdown::render_doc_with(&expanded, &|form, href| {
                crate::links::resolve(
                    cfg,
                    linkspace,
                    &dir,
                    &p.url,
                    &p.locale,
                    &p.rel.to_string_lossy(),
                    form,
                    href,
                )
            })?;
            Ok((&p.key, doc))
        })
        .collect()
}

/// A rendered page body: the expanded fragment plus its Doc (markdown
/// pages) for outline extraction. Computed BEFORE any page is themed so
/// the link graph (q38) can scan every body first.
pub struct PageBody {
    pub(crate) frag: String,
    pub(crate) doc: Option<Doc>,
    /// An unimplemented construct survived expansion; the page is skipped.
    pub(crate) skipped: bool,
}

fn render_page_bodies(
    cfg: &Config,
    db: &SiteDb,
    site: &Site,
    themes: &theme::Themes,
    thumbs: &crate::thumbs::Renditions,
    linkspace: &crate::links::LinkSpace,
) -> Result<HashMap<String, PageBody>> {
    let mut out = HashMap::new();
    for r in &db.routes {
        // `page_bodies` is the PAGE body store, and its being a second store
        // beside the posts one is why `kind` survives I13 at this line: the
        // two are keyed differently (URL here, row key there) and read by
        // different arms of `search_pass` and the feed.
        if r.kind != RouteKind::Page {
            continue;
        }
        let Some(src) = &r.source else { continue };
        // scss compiles in its own pass; it has no body to render.
        if src.extension().is_some_and(|e| e == "scss" || e == "sass") {
            continue;
        }
        let text =
            std::fs::read_to_string(src).with_context(|| format!("reading {}", src.display()))?;
        let (_, body) = split_front_matter(&text);
        // The row first: what the expander renders an embed WITH is the
        // row's theme, not the site default (§5a). A `{% view %}` in a
        // themed page's body arranges its rows the way that page's theme
        // says, exactly as the landing path does.
        let row = r.row.as_ref().and_then(|k| db.rows.get(k));
        let row_thm = themes.get(resolve_theme(themes, r, row.and_then(|p| p.theme.as_deref())).0)?;
        // Expand FIRST, then decide: most pages that look unsupported use
        // only constructs the expander already handles.
        let cx = tags::Ctx {
            includes: Some(cfg.root().join("_includes")),
            site: Some(site),
            thumbs: Some(thumbs),
            theme: Some(row_thm),
            widgets: Some(&cfg.widgets),
            links: Some(linkspace),
            ..tags::Ctx::new(db, &cfg.site.baseurl, src.display().to_string())
        };
        let expanded = tags::expand(body, &cx)?;
        if expanded.contains("{%") {
            out.insert(
                r.url.clone(),
                PageBody {
                    frag: String::new(),
                    doc: None,
                    skipped: true,
                },
            );
            continue;
        }
        // §6a row/view links. Both source shapes resolve through the same
        // closure; they differ only in what walks the document — comrak's AST
        // for markdown, lol_html for raw HTML (§6d stage B).
        let dir = row
            .map(|p| p.rel.parent().map(Path::to_path_buf).unwrap_or_default())
            .unwrap_or_default();
        let locale = row.map(|p| p.locale.as_str()).unwrap_or(&cfg.i18n.default);
        let rel = row
            .map(|p| p.rel.to_string_lossy().to_string())
            .unwrap_or_default();
        let resolve = |form: crate::links::Cite, href: &str| {
            crate::links::resolve(cfg, linkspace, &dir, &r.url, locale, &rel, form, href)
        };
        let (frag, doc) = if src.extension().is_some_and(|e| e == "md") {
            crate::markdown::render_source(&expanded, true, &resolve)?
        } else {
            // One deliberate asymmetry, scoped as tightly as it can be. A
            // raw-HTML body has `{% view %}` expanded INTO it, so where an
            // embed is present the rewriter meets engine-DERIVED URLs beside
            // authored ones and cannot tell them apart — the AST path never
            // had to, because comrak sees an embed as an opaque HtmlBlock and
            // never walks inside one. On those pages a URL already naming a
            // materialized route is left alone instead of being answered with
            // strict's "link the source instead". A page with no embed is all
            // authored, so it gets strict whole. Either way the other strict
            // branch — a link matching nothing at all — fails the build, and
            // catching those is what this seam existed to gain.
            let embeds = body.contains("{% view");
            let raw = |form: crate::links::Cite, href: &str| {
                if embeds && linkspace.is_route(href) {
                    return Ok(None);
                }
                resolve(form, href)
            };
            (crate::rewrite::resolve_links(&expanded, &raw)?, None)
        };
        out.insert(
            r.url.clone(),
            PageBody {
                frag,
                doc,
                skipped: false,
            },
        );
    }
    Ok(out)
}

/// §4 on-demand: publish a row because something referenced it.
///
/// Runs after the write pass, because the references live in FINISHED output.
/// A body alone is not enough — `{% image %}` expands to
/// `<a href='/assets/…'>` so an original is cited by markup that does not
/// exist at load time, and the shell's favicons and stylesheet link are cited
/// by chrome that no body contains.
///
/// **A fixpoint, not a pass.** Materializing a static HTML file or a
/// stylesheet can introduce references of its own. No iteration bound is
/// needed: each round only ever adds rows from a finite set and a row
/// materializes once, so the loop is monotone and terminates structurally.
/// `cited` is the seed AND the output: the caller scanned every finished
/// document once (`citation_map`), this pass reads that as its frontier and
/// appends an entry for each file it materializes, and IO.md §2's
/// `join_citations` then reads the whole of it. One scan of the output serves
/// both consumers, which is what keeps the join's citation edges free.
///
/// **A pull along the graph's edges** (IO.md §5, I10). A citation is a URL and
/// `db.by_url` is the inputs database's address index, so resolving one *is*
/// walking a content edge to the input at its far end — and the test for "have
/// I materialized this already" is the join's own `output` column rather than
/// a private map of pending URLs. That is the whole rewiring, and it is a
/// deletion: this pass used to key a second index off `on_demand && url`, and
/// two indexes of one fact are two things that can disagree. The behaviour is
/// identical by construction — `by_url` holds exactly the rows that carry a
/// URL, and `output` is `None` for an on-demand row until this line sets it.
fn materialize_referenced(
    db: &mut SiteDb,
    out_map: &mut SiteOutput,
    site_url: &str,
    cited: &mut Vec<(String, Vec<String>)>,
) -> Result<usize> {
    // Nothing to pull: no input publishes lazily, so no edge this pass walks
    // can end at an unminted output. Two shapes qualify — an `on_demand` row,
    // whose RULE deferred its route, and an embed-addressed row (IO.md §4a),
    // which has no route to defer and publishes at its strong address.
    if !db.rows.iter().any(|p| {
        p.output.is_none() && (p.strong_url.is_some() || (p.on_demand && !p.url.is_empty()))
    }) {
        return Ok(0);
    }
    // Rung 0 reaches the outputs minted here too (IO.md I10, closing E1's
    // recorded hole): minting is the graph event, so the seam that mints
    // applies the profile's forced fields rather than leaving a route the
    // profile never saw.
    let forced: Vec<(String, grackle_db::filter::Value)> = db
        .forced_fields
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let mut frontier: Vec<String> = cited.iter().flat_map(|(_, c)| c.iter().cloned()).collect();

    let mut made = 0usize;
    while !frontier.is_empty() {
        let mut next: Vec<String> = Vec::new();
        for url in std::mem::take(&mut frontier) {
            // The edge's far end: which INPUT does this citation name?
            //
            // TWO address indexes, because an output has two address slots
            // (IO.md §4a). `by_url` holds CANONICAL row URLs only, so a
            // `/static/{hash}` citation resolves to nothing there — and review
            // I-D named exactly that as the hole: the pull would never publish
            // the bytes, and `join_citations` below would silently drop the
            // asset edge out of the embedding page's `inputs`. `by_strong` is
            // the other half, and it is a MULTI index because identical bytes
            // legitimately share one address.
            for key in resolve_citation(db, &url) {
                let Some(row) = db.rows.get(&key) else {
                    continue;
                };
                // An input that already lands is not the pull's to mint.
                if row.output.is_some() {
                    continue;
                }
                // Where this input publishes, and whether it publishes lazily
                // at all: its strong address if the policy gave it one, else
                // its own URL — and then only if its rule deferred the route.
                let at = match &row.strong_url {
                    Some(s) => s.clone(),
                    None if row.on_demand => row.url.clone(),
                    None => continue, // lands eagerly, or never
                };
                // The twin: another input with the same bytes already
                // published at this address, so there is ONE artifact and this
                // row is its second input. Not a collision — dedupe, which is
                // what a content address is for — so the edge joins the
                // existing output instead of minting a second one over it.
                let at_key = grackle_db::Key::new(&at);
                if let Some(existing) = db.routes.get_mut(&at_key) {
                    existing.inputs.push(key.clone());
                    existing.inputs.sort();
                    existing.inputs.dedup();
                    if let Some(row) = db.rows.get_mut(&key) {
                        row.output = Some(at_key);
                    }
                    continue;
                }
                let path = row.path.clone();
                let strong = row.strong_url.clone();
                let front_mattered = row.front_mattered;
                let bytes = std::fs::read(&path)
                    .with_context(|| format!("on-demand publish: reading {}", path.display()))?;
                // A materialized text file can cite more — and those citations
                // are edges like any other, so they join the map rather than
                // only the frontier.
                if let Ok(text) = std::str::from_utf8(&bytes) {
                    let mine = cited_urls(text, &at, site_url);
                    next.extend(mine.iter().cloned());
                    if !mine.is_empty() {
                        cited.push((at.clone(), mine));
                    }
                }
                let mut route = Route {
                    row: Some(key.clone()),
                    source: Some(path),
                    front_mattered,
                    // IO.md §4a's second address slot on the output side. For
                    // an output the policy minted this equals `url`: the hash
                    // address is where the artifact landed, because no rule
                    // gave it another one.
                    strong_url: strong,
                    // The content edge, minted with the output it points at.
                    inputs: vec![key.clone()],
                    ..Route::new(at.clone(), RouteKind::Object)
                };
                // Rung 0 reaches BOTH shapes this seam mints (IO.md I10, the
                // law at load.rs's `force_route_fields` call site): minting an
                // output is the graph event, so every minting seam applies
                // `SiteDb::forced_fields`. I11 adds a shape to this seam, not
                // a seam — which is the cheapest possible way to stay inside
                // the law, and the reason the strong mint was folded into this
                // loop rather than written beside it.
                for (name, value) in &forced {
                    route.fields.insert(name.clone(), value.clone());
                }
                // IO.md §2, the pull model made literal: a lazily-published
                // row's `output` is `None` for the whole of the build's
                // queryable life and becomes `Some` exactly here — the moment
                // a reference materialized it. "Bare `output` is truthy iff
                // the row lands anywhere" is then true at every instant rather
                // than true of a plan; what it costs is that a filter, which
                // runs upstream of this pass, always sees the unreferenced
                // answer.
                if let Some(row) = db.rows.get_mut(&key) {
                    row.output = Some(route.id.clone());
                }
                db.routes.push(route);
                out_map.insert(at, bytes);
                made += 1;
            }
        }
        frontier = next;
    }
    Ok(made)
}

/// Internal URLs a document cites, via `href`, `src` or CSS `url(...)`,
/// resolved against the URL the document was published at.
///
/// One scanner for both consumers — the backlink graph and on-demand
/// publishing — because they ask the same question. Relative citations are
/// the common case (§6a: a page bundle keeps its screenshots beside it).
/// Citations only — the forward links a human wrote, excluding a listing's
/// spliced `{% view %}` arrangement (§6g Problem 2). The two relation
/// consumers (backlinks and `links_to`) read this; on-demand publishing reads
/// `cited_urls` directly, because an image referenced only by an arrangement
/// still has to be published.
fn cited_urls_cited(text: &str, base_url: &str, site_url: &str) -> Vec<String> {
    cited_urls(&strip_spliced_views(text), base_url, site_url)
}

/// Blank out the innards of every spliced `{% view %}` region so a listing's
/// arrangement links do not read as citations (§6g Problem 2: "Linked from:
/// Home"). The splice is fenced by HTML comment markers the view splicer
/// emits; everything between a matched pair is replaced by spaces (offsets
/// preserved, so nothing else shifts).
fn strip_spliced_views(text: &str) -> String {
    const OPEN: &str = "<!--grackle:view-->";
    const CLOSE: &str = "<!--/grackle:view-->";
    if !text.contains(OPEN) {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(o) = rest.find(OPEN) {
        out.push_str(&rest[..o]);
        let after = &rest[o..];
        match after.find(CLOSE) {
            Some(c) => {
                // Keep the markers (harmless), drop what they wrap.
                out.push_str(OPEN);
                for _ in 0..(c - OPEN.len()) {
                    out.push(' ');
                }
                out.push_str(CLOSE);
                rest = &after[c + CLOSE.len()..];
            }
            None => {
                out.push_str(after);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

fn cited_urls(text: &str, base_url: &str, site_url: &str) -> Vec<String> {
    let dir = match base_url.rfind('/') {
        Some(i) => &base_url[..=i],
        None => "/",
    };
    let mut out = Vec::new();
    let mut push = |u: &str| {
        let u = u.split('#').next().unwrap_or(u);
        let u = u.split('?').next().unwrap_or(u);
        // Our own absolute form is internal; anyone else's is not.
        let u = u.strip_prefix(site_url).unwrap_or(u);
        if u.is_empty() || u.starts_with("//") || u.contains("://") || u.starts_with("data:") {
            return;
        }
        let abs = if u.starts_with('/') {
            u.to_string()
        } else {
            format!("{dir}{u}")
        };
        // Collapse `.` and `..` so `../img/a.png` names the same URL the
        // route map does. The trailing slash survives: a route URL carries
        // one and the backlink graph matches on the whole string.
        let trailing = abs.ends_with('/');
        let mut segs: Vec<&str> = Vec::new();
        for seg in abs.split('/') {
            match seg {
                "" | "." => {}
                ".." => {
                    segs.pop();
                }
                s => segs.push(s),
            }
        }
        let joined = segs.join("/");
        out.push(match (joined.is_empty(), trailing) {
            (true, _) => "/".to_string(),
            (false, true) => format!("/{joined}/"),
            (false, false) => format!("/{joined}"),
        });
    };
    for pat in ["href=\"", "href='", "src=\"", "src='"] {
        let quote = pat.chars().last().unwrap();
        let mut rest = text;
        while let Some(i) = rest.find(pat) {
            let after = &rest[i + pat.len()..];
            let Some(end) = after.find(quote) else { break };
            push(&after[..end]);
            rest = &after[end..];
        }
    }
    let mut rest = text;
    while let Some(i) = rest.find("url(") {
        let after = &rest[i + 4..];
        let Some(end) = after.find(')') else { break };
        push(after[..end].trim().trim_matches(['"', '\'']));
        rest = &after[end..];
    }
    out
}

#[cfg(test)]
mod css_pass_tests {
    use super::*;

    /// `who` names the caller: these run in parallel threads, and a shared
    /// scratch directory means one test compiles another's theme.
    /// (`CARGO_TARGET_TMPDIR` would be tidier, but Cargo defines it only for
    /// integration tests — a unit test gets the system temp dir or nothing.)
    fn compile_as(who: &str, files: &[(&str, &str)]) -> String {
        let dir = std::env::temp_dir().join(format!("grackle-css-pass-{who}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, body) in files {
            std::fs::write(dir.join(name), body).unwrap();
        }
        let mut out = SiteOutput::new();
        let mut stats = Stats::default();
        css_pass(&dir, "", "/css/t.css", None, &mut out, &mut stats).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        String::from_utf8(out.remove("/css/t.css").expect("a sheet is always emitted")).unwrap()
    }

    /// The smallest theme worth having: retune the palette, inherit every
    /// rule. It regressed once — a directory holding only `_tokens.scss`
    /// shipped a stylesheet that never read the file, silently — so the
    /// property is worth an assertion rather than a convention.
    #[test]
    fn a_theme_of_only_tokens_is_compiled() {
        let css = compile_as(
            "tokens-only",
            &[("_tokens.scss", ":root { --measure: 33rem; }")],
        );
        assert!(
            css.contains("33rem"),
            "a tokens-only theme must reach the stylesheet: {css}"
        );
        // It never wrote a sheet, so it never declined the decorative half.
        assert!(css.contains("border-left"), "and inherits the skins too");
    }

    /// The heading ladder is unconditional — a theme never loses it by
    /// writing a stylesheet. This is the fix for the ladder's one growth
    /// cliff, and it is safe because the ladder reads only tokens (a theme
    /// retunes it through `--size`/`--scale`) and was measured inert under
    /// the one theme with a complete type sheet of its own.
    #[test]
    fn every_theme_keeps_the_heading_ladder() {
        for files in [
            vec![("theme.scss", ".x { color: var(--fg); }")],
            vec![("_tokens.scss", ":root { --measure: 33rem; }")],
            vec![],
        ] {
            let css = compile_as(&format!("ladder-{}", files.len()), &files);
            assert!(
                css.contains("text-wrap: balance"),
                "the ladder is not a thing a theme can lose by accident: {css}"
            );
        }
    }

    /// The decorative half still waits to be asked for. Measured: applied
    /// under grack.com the skins move a paragraph 19px and the blog listing
    /// 61px, because a theme with its own opinions about a blockquote will
    /// fight them — which is exactly what the ladder does NOT do.
    #[test]
    fn a_theme_with_a_sheet_is_not_given_the_skins() {
        let css = compile_as(
            "no-imposed-skin",
            &[("theme.scss", ".x { color: var(--fg); }")],
        );
        assert!(css.contains(".x"), "the theme's own rules ship");
        assert!(
            !css.contains("border-left: calc(var(--border) * 3)"),
            "but the base does not impose a blockquote rule: {css}"
        );
    }

    /// §5e's cascade order, declared in full even though `overlay` and
    /// `post` have nothing to emit into them yet: the declaration is what
    /// makes the order authoritative rather than an accident of which
    /// layers happen to exist.
    #[test]
    fn the_full_cascade_order_is_declared() {
        let css = compile_as("layer-order", &[("theme.scss", ".x { color: red; }")]);
        assert!(
            css.contains("@layer reset, base, theme, overlay, post;"),
            "the sheet declares §5e's order: {}",
            &css[..css.len().min(120)]
        );
    }

    /// The reset must keep a long code line from scrolling the whole page,
    /// even for a theme that imports no typography at all — same class of
    /// bug as an image that overflows its column, and `vanilla` is the
    /// theme that would otherwise ship it.
    #[test]
    fn a_wide_code_block_never_scrolls_the_page() {
        let bare = compile_as("pre-bare", &[]);
        assert!(
            bare.contains("overflow-x: auto"),
            "the base reset scrolls `pre` itself: {bare}"
        );
        let themed = compile_as("pre-themed", &[("theme.scss", ".x { color: red; }")]);
        assert!(
            themed.contains("overflow-x: auto"),
            "and so does a theme that imports no typography"
        );
    }
}

#[cfg(test)]
mod cited_url_tests {
    use super::{cited_urls, cited_urls_cited};

    /// §6g Problem 2: an arrangement's links are not citations. The two
    /// clients of one scanner diverge — the backlink/relations view skips a
    /// spliced `{% view %}` region, on-demand publishing keeps it, so an image
    /// only an arrangement references still gets published.
    #[test]
    fn a_spliced_view_is_not_a_citation_but_still_publishes() {
        let html = r#"<a href="/blog/real/">real</a>
            <!--grackle:view--><a href="/blog/listed/">listed</a><img src="/hero.png"><!--/grackle:view-->"#;
        let cited = cited_urls_cited(html, "/", "https://grack.com");
        assert!(cited.contains(&"/blog/real/".to_string()), "{cited:?}");
        assert!(
            !cited.contains(&"/blog/listed/".to_string()),
            "an arrangement link must not count as a citation: {cited:?}"
        );
        // Publishing still sees everything inside the splice.
        let all = cited_urls(html, "/", "https://grack.com");
        assert!(all.contains(&"/blog/listed/".to_string()), "{all:?}");
        assert!(all.contains(&"/hero.png".to_string()), "{all:?}");
    }

    /// The bug this exists to stop, made twice in one session: a scanner that
    /// only accepts root-relative hrefs misses most of a corpus organised as
    /// page bundles (§6a). Measured when it happened: 572 of 838 assets went
    /// unpublished, and the URL-parity check is what caught it.
    #[test]
    fn a_relative_citation_resolves_against_the_citing_url() {
        let refs = cited_urls(
            r#"<img src="screen1.png"><a href="../img/out.png">x</a>"#,
            "/code/legacy/romtool/",
            "https://grack.com",
        );
        assert!(
            refs.contains(&"/code/legacy/romtool/screen1.png".to_string()),
            "{refs:?}"
        );
        assert!(
            refs.contains(&"/code/legacy/img/out.png".to_string()),
            "{refs:?}"
        );
    }

    /// Our own absolute form is internal, anyone else's is not, and a
    /// fragment or query is not part of the target.
    #[test]
    fn site_absolute_is_internal_and_foreign_absolute_is_not() {
        let mut got = cited_urls(
            r##"<a href="/blog/x/">x</a> <a href='/a.png'>i</a>
                <a href="https://grack.com/blog/y/#frag">abs</a>
                <a href="https://elsewhere.com/z">ext</a>
                <a href="//cdn.example/w">rel</a> <a href="#top">frag</a>"##,
            "/page/",
            "https://grack.com",
        );
        got.sort();
        assert_eq!(got, vec!["/a.png", "/blog/x/", "/blog/y/"]);
    }

    #[test]
    fn root_relative_and_css_url_and_externals() {
        let refs = cited_urls(
            r#"<img src="/a/b.png">@font-face{src:url('/css/f.woff2')}
               <a href="https://e.com/x.png">e</a><img src="//cdn/y.png">"#,
            "/page/",
            "https://grack.com",
        );
        assert!(refs.contains(&"/a/b.png".to_string()), "{refs:?}");
        assert!(refs.contains(&"/css/f.woff2".to_string()), "{refs:?}");
        assert!(
            !refs
                .iter()
                .any(|r| r.contains("e.com") || r.contains("cdn")),
            "external citations must not be treated as ours: {refs:?}"
        );
    }
}

/// One backlink: `(source title, source url, source date)`. The date rides
/// along so a backlink list can be read in date order (q38).
pub(crate) type Backlink = (String, String, Option<chrono::NaiveDate>);

/// The link graph, both directions, from one scan. `linked_from` inverts the
/// citations (who points here); `links_to` keeps them forward (where this
/// points) — the derived relation names §6g's `related` default reads to
/// avoid re-showing a page you already linked. Both are the *citation* view of
/// the graph: a listing's spliced arrangement links are not citations (§6g
/// Problem 2), so both skip the splice — unlike on-demand publishing, which
/// scans with `cited_urls` directly and must still see them.
fn backlinks_map(
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
                p.date,
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

/// The searchable projection of the posts table — the CLI smoke query
/// (`grackle query search`), which runs no render pass and feeds raw
/// markdown. The SHIPPED index is not this: it is the `shell = "search"`
/// view's serialization (see `search_pass`), which may span tables.
pub fn search_docs(
    db: &SiteDb,
    html_of: impl Fn(&Row) -> String,
) -> Vec<grackle_search_core::SearchDoc> {
    db.posts()
        .map(|p| grackle_search_core::SearchDoc {
            url: p.url.clone(),
            title: p.title.clone().unwrap_or_else(|| p.url.clone()),
            date: p.date.map(crate::db::pretty_date).unwrap_or_default(),
            html: html_of(p),
            tags: p.tags.clone(),
        })
        .collect()
}

/// Run a registered script shell (§5g): `sh -c command` from the site root,
/// JSON on stdin, bytes on stdout. Non-zero exit is a build error carrying
/// stderr — a script shell fails loud, like everything else. Stdin is fed
/// from a thread so a script that streams output before draining its input
/// can't deadlock against the pipe buffer.
fn run_script_shell(root: &Path, command: &str, payload: &serde_json::Value) -> Result<Vec<u8>> {
    use std::io::Write;
    use std::process::{Command as Proc, Stdio};
    let mut child = Proc::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn failed")?;
    let mut stdin = child.stdin.take().expect("stdin was piped");
    let data = serde_json::to_vec(payload)?;
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&data);
    });
    let out = child.wait_with_output()?;
    let _ = writer.join();
    if !out.status.success() {
        anyhow::bail!(
            "exited {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(out.stdout)
}

/// Search (§6b, §5g): the index is a SHELL — a view declares
/// `shell = "search"` with a filter over the route schema (the sitemap's
/// shape), and the rows that pass are the searchable set, serialized as
/// postcard at the view's route. Posts and pages carry bodies; other route
/// kinds are silently unsearchable even if the filter admits them. The
/// wasm consumer + /search.js loader are engine assets embedded in the
/// binary (they must version with the index format), emitted only when a
/// search view exists, fetched only when a theme's trigger is clicked.
fn search_pass(
    cfg: &Config,
    db: &SiteDb,
    bodies: &HashMap<&grackle_db::Key, Doc>,
    page_bodies: &HashMap<String, PageBody>,
    out_map: &mut SiteOutput,
    stats: &mut Stats,
) -> Result<()> {
    let mut any = false;
    for fold in &db.routes {
        let Some(view) = &fold.view else { continue };
        let Some(v) = cfg.views.get(view) else {
            continue;
        };
        if v.shell.as_deref() != Some("search") {
            continue;
        }
        let route = &fold.url;
        // Resolved at load, like the sitemap's.
        let docs: Vec<grackle_search_core::SearchDoc> = fold
            .route_members
            .iter()
            .filter_map(|k| db.routes.get(k))
            // The two arms are two BODY STORES, not two kinds of thing to say
            // about an output — which is why `kind` survives I13 here: a post's
            // html is in `bodies` (keyed by row) and a page's in `page_bodies`
            // (keyed by URL), and no fact on the route says which pass filled
            // which. `_ => None` is the rest: a byte copy has no body to
            // search, and a fold's output is not a document.
            .filter_map(|r| match r.kind {
                crate::db::RouteKind::Post => {
                    r.row.as_ref().and_then(|k| db.rows.get(k)).map(|p| {
                        grackle_search_core::SearchDoc {
                            url: p.url.clone(),
                            title: p.title.clone().unwrap_or_else(|| p.url.clone()),
                            date: p.date.map(crate::db::pretty_date).unwrap_or_default(),
                            html: bodies
                                .get(&p.key)
                                .map(|d| d.whole.clone())
                                .unwrap_or_default(),
                            tags: p.tags.clone(),
                        }
                    })
                }
                crate::db::RouteKind::Page => {
                    let pb = page_bodies.get(&r.url).filter(|pb| !pb.skipped)?;
                    let p = r.row.as_ref().and_then(|k| db.rows.get(k))?;
                    Some(grackle_search_core::SearchDoc {
                        url: p.url.clone(),
                        // A titleless page is still searchable by body; its
                        // URL is the only honest label a hit can wear.
                        title: p.title.clone().unwrap_or_else(|| p.url.clone()),
                        date: p.date.map(crate::db::pretty_date).unwrap_or_default(),
                        // Markdown pages searched from the same bytes that
                        // ship; raw-HTML pages from their body fragment.
                        html: pb
                            .doc
                            .as_ref()
                            .map(|d| d.whole.clone())
                            .unwrap_or_else(|| pb.frag.clone()),
                        tags: p.tags.clone(),
                    })
                }
                _ => None,
            })
            .collect();
        let t = std::time::Instant::now();
        let (index, st) = grackle_search_core::build_index(&docs);
        let bin = index.to_bytes();
        stats.search_bytes = bin.len();
        println!(
            "  search    {} docs, {} terms, {} postings -> {} KB in {:.0}ms",
            st.docs,
            st.terms,
            st.postings,
            bin.len() / 1024,
            t.elapsed().as_secs_f64() * 1000.0
        );
        out_map.insert(route.clone(), bin);
        any = true;
    }
    if any {
        out_map.insert(
            "/search.js".to_string(),
            include_bytes!("../assets/search.js").to_vec(),
        );
        out_map.insert(
            "/search.wasm".to_string(),
            include_bytes!("../assets/search.wasm").to_vec(),
        );
    }
    Ok(())
}

/// `@charset` is only legal as the very first thing in a stylesheet, and
/// grass emits one per compilation unit — two of which now go INSIDE layer
/// blocks. Strip them there and write one at the top of the file.
fn strip_charset(css: &str) -> &str {
    css.strip_prefix("@charset \"UTF-8\";\n").unwrap_or(css)
}

/// A theme's stylesheet is the ENGINE BASE plus whatever the theme adds
/// (§5e) — compiled to the URL the theme's pages link (`default` keeps
/// /css/main.css for parity).
///
/// **The theme's own sheet is `theme.scss`, or failing that `_tokens.scss`
/// alone.** The second case is the smallest theme worth having: retune the
/// palette and the measure, inherit every rule. Compiling it needs saying
/// out loud, because the alternative is the failure mode it replaced — a
/// directory holding one `_tokens.scss` shipped a stylesheet that never
/// read it, and nothing said so. A file that is silently ignored is worse
/// than one that errors.
///
/// The two sheets arrive in declared layers, the cascade order §5e states.
/// That buys what plain concatenation cannot: a theme's rule wins over the
/// base's whatever the selectors say, so a theme writing `.crumb` is never
/// outranked by the base's `[data-kind="crumb"] + [data-kind="crumb"]`.
///
/// **These per-theme sheets ARE the megacss** (IO.md §6, item I5). The model
/// is one CSS artifact — engine base, theme, site overlay, extracted
/// `root.html` styles, eventually per-post styles — and chunking it per theme
/// is an optimization of that one artifact, not a competing design: a page
/// links exactly one sheet, and the sheet it links is the whole cascade for
/// that page. Nothing about the URLs or the assembly changed when the model
/// said so; what changed is that "the megacss" now names something that
/// exists.
///
/// `head_style` is the theme root's `<head>` CSS (`Theme::head_style`), and
/// it lands in the THEME layer **after** `theme.scss`. Two reasons, and the
/// second is why it is not merely arbitrary: a theme's files are read top to
/// bottom by whoever maintains it, and `root.html` is the file that states
/// the theme's own frame — so a rule it writes about its own chrome should
/// win against the general sheet, not lose to it. It is also the reading that
/// preserves I4's inline emission: a `<style>` last in a `<head>` outranked
/// the stylesheet link above it, and staying last keeps the same rule
/// winning after the move.
fn css_pass(
    theme_dir: &Path,
    head_style: &str,
    url: &str,
    overlay: Option<&str>,
    out_map: &mut SiteOutput,
    stats: &mut Stats,
) -> Result<()> {
    // `theme.scss` if the theme wrote one, else `_tokens.scss` on its own.
    // `wants_skin` is the ONLY thing a sheet's presence now decides: the
    // heading ladder and block rhythm always apply (measured inert under a
    // theme that has its own — see `base::css`), and only the decorative
    // skins wait to be asked for. That shrinks the ladder's one
    // discontinuity from "the whole page changes" to "the code panel and
    // blockquote rule are missing".
    let full = theme_dir.join("theme.scss");
    let tokens = theme_dir.join("_tokens.scss");
    let (own, wants_skin) = match (full.exists(), tokens.exists()) {
        (true, _) => (Some(full), false),
        (false, true) => (Some(tokens.clone()), true),
        (false, false) => (None, true),
    };

    // The full cascade order §5e declares, not just the two layers used
    // today. `overlay` (§5b subtree styles) and `post` (§6c per-post
    // styles) are unbuilt — declaring them now is free and makes this
    // statement the authority on the order, so whoever builds them slots
    // in rather than discovering that an undeclared layer sorts last by
    // accident. `reset` is the base's own reset partial, which currently
    // ships inside `base`.
    let mut css = format!(
        "@charset \"UTF-8\";\n@layer reset, base, theme, overlay, post;\n\
         @layer base {{\n{}\n}}\n",
        strip_charset(crate::base::css(wants_skin))
    );
    // The theme layer's contents, in order: the theme's own sheet, then the
    // CSS its `root.html` head declared. Collected rather than appended
    // straight to `css` so the layer block is emitted once, and only when
    // something reached it — a theme with neither writes no `@layer theme`
    // at all, exactly as before this item.
    let mut theme_layer: Vec<String> = Vec::new();
    // Every partial ANY of the theme's CSS sources pulled in, both passes
    // pooled. The orphaned-tokens question below is about the theme as a
    // whole — "does anything the theme compiles read this file?" — and a
    // per-pass list can only answer it for one file (IR5).
    let mut imported: Vec<String> = Vec::new();
    // A tokens-only theme (`_tokens.scss`, no `theme.scss`) reads its tokens
    // by BEING them: `own` is the partial itself, so no `@import` names it
    // and none could. It is the one shape where the file is fully alive and
    // the import list is empty.
    let tokens_only = own.as_deref() == Some(tokens.as_path());
    if let Some(src) = own {
        let text = std::fs::read_to_string(&src)?;
        let (_, body) = split_front_matter(&text);
        let mut seen = Vec::new();
        let flat = inline_imports(body, theme_dir, &mut seen)?;
        imported.append(&mut seen);

        let opts = grass::Options::default().load_path(theme_dir);
        match grass::from_string(flat, &opts) {
            Ok(theme_css) => theme_layer.push(strip_charset(&theme_css).to_string()),
            // Reported here so `serve` shows it immediately, and RECORDED so
            // `build` can refuse: the binder treats a malformed fragment as
            // a build error with file:line, and the CSS half of the same
            // theme should not be the lenient one. Publishing a site whose
            // stylesheet silently failed to compile is the worst outcome
            // available — it looks deployable and is wrong.
            Err(e) => {
                eprintln!("scss: {}: {e}", src.display());
                stats.css_errors.push(format!("{}: {e}", src.display()));
            }
        }
    }
    // The theme root's head styles (IO.md §6), through the SAME pipeline as
    // `theme.scss`: `@import` inlining, then grass, with the theme directory
    // on the load path.
    //
    // **Compiled, not passed through** — decided at I5. A `root.html` head is
    // authored as CSS in an HTML file, and plain CSS is valid SCSS, so
    // compiling costs a pass over a few lines and buys the author the two
    // things the rest of the theme already has: nesting, and
    // `@import "tokens";` reaching the theme's own partial or the engine
    // base's. The alternative — verbatim — would have made one file in a
    // theme the file where the theme's own vocabulary does not work, which is
    // the kind of exception that is only ever discovered by hitting it.
    // A style that does not compile is the same event as a `theme.scss` that
    // does not: reported, recorded, and a publishing build refuses.
    if !head_style.is_empty() {
        let root_html = theme_dir.join("root.html");
        let mut seen = Vec::new();
        let flat = inline_imports(head_style, theme_dir, &mut seen)?;
        imported.append(&mut seen);
        let opts = grass::Options::default().load_path(theme_dir);
        match grass::from_string(flat, &opts) {
            Ok(head_css) => theme_layer.push(strip_charset(&head_css).to_string()),
            Err(e) => {
                eprintln!("scss: {}: {e}", root_html.display());
                stats
                    .css_errors
                    .push(format!("{}: {e}", root_html.display()));
            }
        }
    }
    // A `_tokens.scss` nobody imports is the dead-file trap again, one arm
    // along: the sheet compiles, the tokens are simply never read, and the
    // only symptom is a theme that ignores its own palette. Worth a word, not
    // a failure, because a theme may legitimately keep a partial it does not
    // use yet.
    //
    // **Asked here, of the whole theme** (IR5). It used to be asked inside the
    // `theme.scss` pass, of that pass's imports alone, and was therefore false
    // in the two shapes where the tokens are read by something else: a
    // tokens-only theme (they ARE the sheet — a wart of this warning's own
    // vintage), and a theme whose `root.html` head imports them while
    // `theme.scss` does not (I5 gave the head its own pass and its own list).
    // What survives is the case the warning was written for: a `theme.scss`
    // beside a `_tokens.scss` that nothing in the theme pulls in.
    if tokens.exists() && !tokens_only && !imported.iter().any(|s| s == "tokens") {
        let w = format!(
            "{} has a _tokens.scss that nothing imports — add `@import \
             \"tokens\";` to theme.scss, or the palette is dead weight",
            theme_dir.display()
        );
        eprintln!("grackle: {w}");
        stats.css_warnings.push(w);
    }
    if !theme_layer.is_empty() {
        css.push_str(&format!(
            "@layer theme {{\n{}\n}}\n",
            theme_layer.join("\n")
        ));
    }
    // §5b rung 1: the site's own sheet, above every theme's. Appended to each
    // theme's stylesheet rather than served separately, because it must apply
    // whichever theme is active — that is the whole guarantee, that a knob set
    // here survives a theme SWITCH and not merely a theme update.
    if let Some(o) = overlay {
        css.push_str(&format!("@layer overlay {{\n{}\n}}\n", strip_charset(o)));
    }
    stats.css += css.len();
    out_map.insert(url.to_string(), css.into_bytes());
    Ok(())
}

/// The site's own stylesheet: `.style.scss` at the root, compiled once and
/// handed to every theme's sheet (§5b, rung 1 of themes/DESIGN.md §2).
///
/// The cheapest real customization there is, and the one the ladder promised
/// and could not deliver: `:root { --accent: … }` in a file the site owns,
/// landing in the `overlay` layer above theme CSS. Because the token names are
/// a cross-theme contract, an override written here survives switching themes,
/// not just updating one — which is what makes this a rung below "derive a
/// theme" rather than a worse way to do it.
///
/// Positional `.style.scss` (§5b's other half — a file per subtree, scoped by
/// `data-scope`) is NOT this. It needs every rendered row to carry its scope
/// chain, and nothing emits one yet.
fn site_overlay(root: &Path, stats: &mut Stats) -> Option<String> {
    let src = root.join(".style.scss");
    let text = std::fs::read_to_string(&src).ok()?;
    // Unscoped, so `:root` works here — which is the point of the root file and
    // exactly what §5b warns is impossible in a SCOPED one, where a `:root`
    // block would be nested inside a selector and silently never apply.
    match grass::from_string(text, &grass::Options::default().load_path(root)) {
        Ok(css) => Some(css),
        Err(e) => {
            eprintln!("scss: {}: {e}", src.display());
            stats.css_errors.push(format!("{}: {e}", src.display()));
            None
        }
    }
}

/// The collection at the base of a view's `from` chain — whose role (read off
/// its `source`, now that `kind` is gone) decides which render pass owns the
/// view's routes. None for a fold over every output, which has no collection
/// under it (IO.md §4).
fn view_base_collection<'a>(cfg: &'a Config, view: &str) -> Option<&'a Collection> {
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
            x.view == r.view && x.page.is_some() && x.locale == r.locale && x.params == r.params
        })
        .collect();
    siblings.sort_by_key(|x| x.page);
    let urls: Vec<String> = siblings.iter().map(|x| x.url.clone()).collect();
    Ok(parts::pagination(cur, &urls))
}

/// Section outline for a row inside a `.section` unit (§6e), cached per unit.
fn section_parts(
    db: &SiteDb,
    section_trees: &mut HashMap<PathBuf, Vec<crate::outline::Node>>,
    rel: &Path,
    page_url: &str,
    default_locale: &str,
) -> Vec<parts::PartMap> {
    crate::outline::nearest(&db.sections, rel)
        .map(|sec| {
            let tree = section_trees
                .entry(sec.to_path_buf())
                .or_insert_with(|| crate::outline::section_tree(db, sec, default_locale));
            crate::outline::to_parts(tree, page_url)
        })
        .unwrap_or_default()
}

/// Locale twins of `row` (other locales, labelled by language).
fn locale_twins(db: &SiteDb, p: &Row) -> Vec<(String, String)> {
    db.by_logical
        .get(&p.logical)
        .map(|sibs| {
            sibs.iter()
                .filter_map(|k| db.rows.get(k))
                .filter(|s| s.url != p.url)
                .map(|s| (s.locale.clone(), s.url.clone()))
                .collect()
        })
        .unwrap_or_default()
}

/// Post body if held, else non-skipped page body.
fn row_body_html<'a>(
    p: &Row,
    bodies: &'a HashMap<&grackle_model::Key, Doc>,
    page_bodies: &'a HashMap<String, PageBody>,
) -> Option<&'a str> {
    bodies
        .get(&p.key)
        .map(|d| d.whole.as_str())
        .or_else(|| {
            page_bodies
                .get(&p.url)
                .filter(|pb| !pb.skipped)
                .map(|pb| pb.frag.as_str())
        })
}

/// Members of a view/route as previews — objects, truncated prose, or tree
/// bodies from `page_bodies` when the post map has none.
pub(crate) fn member_previews<'a>(
    cfg: &Config,
    db: &'a crate::db::SiteDb,
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
            row_preview(cfg, p, thumbs, Some(html), truncated)
        })
        .collect()
}

/// An object row as a preview: the row IS the picture, so it is its own
/// thumbnail source and its stem is the only label it has. `row` stays unset
/// — an object has no date, tags or prose to answer with.
pub(crate) fn object_preview<'a>(
    o: &crate::db::Row,
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
    cfg: &Config,
    p: &'a crate::db::Row,
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
        tags: parts::pill_stream(cfg, p, "tags"),
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
        if let Some(field) = chain.last().map(|s| crate::db::spec_field(s)) {
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

/// Resolve `@import "name"` textually against `_sass/_name.scss`, recursively.
///
/// `grass` rejects a **nested** `@import` ("this at-rule is not allowed here"),
/// but `_sass/_post.scss:240` has one — `pre > code { @import "rouge"; }` — to
/// scope Rouge's syntax classes. libsass (what Jekyll uses) allows it, so the
/// site is legal input that grass will not take. Inlining first gives grass the
/// flattened source it wants without touching the site's sass.
fn inline_imports(src: &str, load: &Path, seen: &mut Vec<String>) -> Result<String> {
    let mut out = String::with_capacity(src.len() * 2);
    for line in src.lines() {
        let t = line.trim();
        let name = t
            .strip_prefix("@import")
            .map(|r| r.trim().trim_end_matches(';').trim())
            .filter(|r| r.starts_with('"') && r.ends_with('"') && !r.contains("url("))
            .map(|r| r.trim_matches('"'));
        let Some(name) = name else {
            out.push_str(line);
            out.push('\n');
            continue;
        };
        if seen.iter().any(|s| s == name) {
            continue; // already inlined; sass imports are idempotent here
        }
        // The theme's own partial wins; failing that, the engine base's, so
        // a theme can `@import "tokens"` to build on the base vocabulary
        // without carrying a copy of it. Neither: leave the line for grass,
        // which will say so.
        let path = load.join(format!("_{name}.scss"));
        let inner = if path.exists() {
            std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?
        } else if let Some(src) = crate::base::partial(name) {
            src.to_string()
        } else {
            out.push_str(line);
            out.push('\n');
            continue;
        };
        seen.push(name.to_string());
        // Preserve the indentation so nested imports stay inside their block.
        let indent = &line[..line.len() - line.trim_start().len()];
        for l in inline_imports(&inner, load, seen)?.lines() {
            out.push_str(indent);
            out.push_str(l);
            out.push('\n');
        }
    }
    Ok(out)
}

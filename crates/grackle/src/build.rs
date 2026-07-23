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

use crate::config::{Config, Kind, View};
use crate::db::{Route, RouteKind, Row, SiteDb};
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
) -> Vec<(String, String)> {
    if twins.is_empty() {
        return Vec::new();
    }
    let mut v = vec![(self_locale.to_string(), format!("{site_url}{self_url}"))];
    v.extend(
        twins
            .iter()
            .map(|(loc, url)| (loc.clone(), format!("{site_url}{url}"))),
    );
    v
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
        assert_eq!(
            locale_alternates("https://s", "en", "/a/", &twins),
            vec![
                ("en".to_string(), "https://s/a/".to_string()),
                ("fr".to_string(), "https://s/fr/a/".to_string()),
            ]
        );
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
    let _ = std::fs::remove_dir_all(out);
    std::fs::create_dir_all(out)?;
    for (url, bytes) in &map {
        write(&out_path(out, url), bytes)?;
    }
    Ok(stats)
}

/// Render every routable URL into memory. Writes nothing to the output; the
/// only disk it touches is the content-addressed `_cache/` (thumbnails, §6b).
pub fn render_site(cfg: &Config, db: &mut SiteDb) -> Result<(SiteOutput, Stats)> {
    let mut out_map: SiteOutput = BTreeMap::new();

    let site = Site {
        url: &cfg.site.url,
        title: &cfg.site.title,
        author: &cfg.site.author,
        email: cfg.site.email.as_deref(),
        noindex: cfg.site.noindex,
    };
    let profile = cfg.profile.as_deref();
    // Each theme compiles its own stylesheet; `default` keeps /css/main.css
    // (URL parity with the reference), others get /css/{name}.css.
    let css_of = |theme: Option<&str>| match theme {
        None | Some("default") => format!("{}/css/main.css", cfg.site.baseurl),
        Some(n) => format!("{}/css/{n}.css", cfg.site.baseurl),
    };

    let mut stats = Stats::default();

    let root = cfg.root();
    let theme_dir = root.join("themes/default");

    let thumbs = thumbs_pass(cfg, db, &root, &mut out_map, &mut stats)?;

    // An image field's published URL (§5e image parts): the thumbnail's when
    // the pass generated one, else the original under baseurl. This is the
    // presentation `fill_from_fields` delegates so it need not know either.
    let resolve_asset = |src: &str| -> String {
        thumbs
            .get(src)
            .map(|t| t.url.clone())
            .unwrap_or_else(|| asset_url(&cfg.site.baseurl, src))
    };

    // ---- themes: every directory under themes/, loaded once (§5e). All
    // theme errors — malformed fragment, unknown slot, arity violation —
    // surface here, before anything renders. Theme is chosen per ROW (§5a).
    // §5e: the part vocabulary this build runs against — the engine's kinds
    // plus whatever `[[parts]]` the site declares. Fragments are checked
    // against it, so a theme can place a part the site invented.
    let schemas = parts::Schemas::load(cfg)?;
    let themes =
        theme::Themes::load_all(&root.join("themes"), &root, &schemas).context("loading themes")?;
    let thm = themes.get(None)?;

    // §6a row/view links: the resolution space, once per build.
    let linkspace = crate::links::LinkSpace::new(cfg, db, &root);
    let bodies = render_bodies(cfg, db, &thumbs, &linkspace)?;
    let page_bodies = render_page_bodies(cfg, db, &site, thm, &thumbs, &linkspace)?;

    // ---- the link graph (q38): scan every rendered body once — posts and
    // pages alike — and invert. Backlinks are one more relations axis; the
    // scan reads the same bytes that ship, so link and index cannot desync.
    let backlinks = backlinks_map(db, &bodies, &page_bodies, &cfg.site.url);

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
    let related = crate::embed::rank(db, &loaded.vectors, &cfg.related);
    stats.embed_pending = loaded.pending;

    // ---- posts: document parts -> theme fragments -> shell
    // Posts only. `bodies` holds the in-memory bodies the posts loader
    // produced; a tree row is rendered by the `RouteKind::Page` arm below.
    let rendered: Vec<(String, String)> = db
        .post_ix
        .par_iter()
        .filter_map(|k| db.rows.get(k))
        .map(|p| -> Result<(String, String)> {
            let head = render::head_for_post(p, &site);
            let trail = crate::trails::post_trail(cfg, db, p);
            let whole = bodies[p.url.as_str()].whole.as_str();
            let rel: Vec<crate::db::Key> = db
                .by_url
                .get(&p.url)
                .and_then(|k| related.by_post.get(k))
                .map(|v| v.iter().map(|(k, _)| k.clone()).collect())
                .unwrap_or_default();
            // §6e heading axis: `toc:` rows carry their outline, extracted
            // from the same rendered bytes. h2–h3 is the v1 depth window
            // (production policy, not CSS — never ship what a theme hides).
            let outline = if p.toc {
                let tree = crate::outline::heading_tree(&bodies[p.url.as_str()].headings(), 2, 3);
                crate::outline::to_parts(&tree, &p.url)
            } else {
                Vec::new()
            };
            let bl = backlinks.get(&p.url).map(Vec::as_slice).unwrap_or(&[]);
            // §6f: this row in other locales, labelled by language.
            let translations: Vec<(String, String)> = db
                .by_logical
                .get(&p.logical)
                .map(|sibs| {
                    sibs.iter()
                        .filter_map(|k| db.rows.get(k))
                        .filter(|s| s.url != p.url)
                        .map(|s| (s.locale.clone(), s.url.clone()))
                        .collect()
                })
                .unwrap_or_default();
            let mut head = head;
            head.alternates = locale_alternates(&cfg.site.url, &p.locale, &p.url, &translations);
            let mut doc =
                parts::document(cfg, db, p, whole, trail, &rel, bl, outline, &translations);
            parts::fill_from_fields(&mut doc, p, &schemas, &resolve_asset)?;
            let main = thm.fragments.render(&doc);
            let dir = p.path.parent().unwrap_or(&root);
            // Theme is per ROW (§5a), posts included.
            let (theme_name, subtheme) = match p.theme.as_deref() {
                Some(spec) => {
                    let (n, s) = theme::split_spec(spec);
                    (Some(n), s)
                }
                None => (None, None),
            };
            let row_thm = themes.get(theme_name)?;
            let html = row_thm.page(
                render::head_html(&head, &css_of(theme_name)),
                &cfg.site.title,
                main,
                dir,
                &p.locale,
                &fill_link_resolver(cfg, &linkspace, &p.locale),
                subtheme.as_deref(),
                profile,
            )?;
            Ok((p.url.clone(), html))
        })
        .collect::<Result<Vec<_>>>()?;
    for (url, html) in rendered {
        out_map.insert(url, html.into_bytes());
        stats.posts += 1;
    }

    // ---- one walk of the route table, dispatched by layout (§9b).
    //
    // Each pass lives in `passes/` and states which layout it renders; a route
    // matches at most one. Passes read `Ctx` and cannot see each other's
    // output, so their order carries no meaning.
    {
        let ctx = crate::passes::Ctx {
            cfg,
            db,
            site: &site,
            themes: &themes,
            thm,
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
        let Some(content) = v.content.as_deref() else {
            continue;
        };
        if r.kind != RouteKind::View {
            continue;
        }
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

        // This route's slice, by the view's base kind.
        let embed_parts = match view_base_kind(cfg, view) {
            Some(Kind::Posts) => {
                let summary_field = cfg.fields_for(view).get("summary").and_then(|f| f.truncate);
                let items: Vec<parts::Preview> = r
                    .members
                    .iter()
                    .filter_map(|k| db.rows.get(k))
                    .map(|p| {
                        let (html, truncated) = match bodies.get(p.url.as_str()) {
                            Some(d) => match summary_field {
                                Some(t) => d.truncate(t.max_blocks, t.max_chars),
                                None => (d.whole.clone(), false),
                            },
                            None => (String::new(), false),
                        };
                        row_preview(cfg, p, &thumbs, Some(html), truncated)
                    })
                    .collect();
                let pagination = pagination_parts(db, view, v, r)?;
                parts::listing_embed(items, false, pagination)
            }
            Some(Kind::Tree) => {
                let items: Vec<parts::Preview> = r
                    .members
                    .iter()
                    .filter_map(|k| db.rows.get(k))
                    .map(|p| row_preview(cfg, p, &thumbs, None, false))
                    .collect();
                parts::listing_embed(items, v.featured, None)
            }
            Some(Kind::Objects) => {
                let items: Vec<parts::Preview> = r
                    .members
                    .iter()
                    .filter_map(|k| db.rows.get(k))
                    .map(|o| object_preview(o, &thumbs))
                    .collect();
                parts::listing_embed(items, false, None)
            }
            None => continue,
        };

        // The row's theme renders both the slice and the page (§5a: the
        // landing wears its section's clothes).
        let (theme_name, subtheme) = match row.theme.as_deref() {
            Some(spec) => {
                let (n, s) = theme::split_spec(spec);
                (Some(n), s)
            }
            None => (None, None),
        };
        let row_thm = themes.get(theme_name)?;
        let embed_html = row_thm
            .fragments
            .render_with(&embed_parts, v.variant.as_deref());

        // Must-place (q45): the claimed row owns the arrangement — a body
        // that never places the owner's embed strands the view's rows.
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
        let resolve =
            |href: &str| crate::links::resolve(cfg, &linkspace, &dir, &r.url, loc, &rel, href);
        let frag = if src.extension().is_some_and(|e| e == "md") {
            crate::markdown::render_doc_with(&expanded, &resolve)?
                .whole
                .clone()
        } else {
            crate::rewrite::resolve_links(&expanded, &resolve)?
        };
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

        // The landing per locale IS the translation set — computed from
        // the owner's materialized routes, not from which prose variants
        // exist (a fallback landing is still the French landing).
        let translations: Vec<(String, String)> = db
            .routes
            .iter()
            .filter(|x| {
                x.kind == RouteKind::View
                    && x.view.as_deref() == Some(view.as_str())
                    && x.key.is_none()
                    && x.page.is_none_or(|n| n == 1)
                    && x.url != r.url
            })
            .map(|x| {
                let l = x.locale.as_deref().unwrap_or(&cfg.i18n.default);
                (cfg.i18n.name_of(l).to_string(), x.url.clone())
            })
            .collect();

        let section: Vec<parts::PartMap> = crate::outline::nearest(&db.sections, &row.rel)
            .map(|sec| {
                let tree = section_trees
                    .entry(sec.to_path_buf())
                    .or_insert_with(|| crate::outline::section_tree(db, sec, &cfg.i18n.default));
                crate::outline::to_parts(tree, &r.url)
            })
            .unwrap_or_default();
        let bl = backlinks.get(&r.url).map(Vec::as_slice).unwrap_or(&[]);

        let main = match row.layout.as_deref() {
            Some("page") | Some("post") => {
                let mut doc = parts::document_tree(
                    cfg,
                    loc,
                    &crate::trails::home_url(cfg, db, loc),
                    &title,
                    &r.url,
                    parts::TreeDoc {
                        ancestors: &crate::trails::ancestors(cfg, db, &r.url),
                        section,
                        outline: Vec::new(),
                        hero: None,
                        backlinks: bl,
                        translations: &translations,
                    },
                    &frag,
                );
                parts::fill_from_fields(&mut doc, row, &schemas, &resolve_asset)?;
                row_thm.fragments.render(&doc)
            }
            _ => frag.clone(),
        };
        let head = render::head_simple(&title, &r.url, &site, false);
        let dir = src.parent().unwrap_or(&root);
        let html = row_thm.page(
            render::head_html(&head, &css_of(theme_name)),
            &cfg.site.title,
            main,
            dir,
            loc,
            &fill_link_resolver(cfg, &linkspace, loc),
            subtheme.as_deref(),
            profile,
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
            .map(|p| {
                let html = bodies
                    .get(p.url.as_str())
                    .map(|d| d.whole.as_str())
                    .or_else(|| {
                        page_bodies
                            .get(&p.url)
                            .filter(|pb| !pb.skipped)
                            .map(|pb| pb.frag.as_str())
                    })
                    .unwrap_or("");
                (p, html)
            })
            .collect();
        let xml = render::feed(&site, &r.url, &updated, &entries);
        out_map.insert(r.url.clone(), xml.into_bytes());
        stats.serialized += 1;
    }

    // ---- sitemap: `over = "*"` views serialize the finished route set.
    //
    // The star view (§5) counted its matches at load; here we re-run the same
    // filter to enumerate them. `lastmod` is emitted only for posts, from the
    // content date. jekyll-sitemap also stamps static files with their file
    // *mtime* — but that is checkout-time noise (every clone differs) and works
    // against the indexing goal this whole project exists for, so it is
    // deliberately dropped — the URL *set* is unaffected. (DESIGN §4a is the
    // related draft/hidden concern.)
    for star in &db.routes {
        let Some(view) = &star.view else { continue };
        let Some(v) = cfg.views.get(view) else {
            continue;
        };
        // The sitemap SHELL, likewise declared.
        if v.shell.as_deref() != Some("sitemap") {
            continue;
        }
        // Resolved at load like every other view's, rather than re-derived
        // from the filter's source text here.
        let entries: Vec<(String, Option<String>)> = star
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
        out_map.insert(star.url.clone(), xml.into_bytes());
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
        let rows: Vec<serde_json::Value> = match view_base_kind(cfg, view) {
            Some(Kind::Tree) => r
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
                        "html": page_bodies.get(&p.url).map(|pb| pb.frag.as_str()).unwrap_or(""),
                    })
                })
                .collect(),
            _ => r
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
                        "html": bodies.get(p.url.as_str()).map(|d| d.whole.as_str()).unwrap_or(""),
                    })
                })
                .collect(),
        };
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
                let row = db.by_url.get(r.url.as_str()).and_then(|k| db.rows.get(k));
                let layout = row.and_then(|p| p.layout.as_deref());
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
                let outline = match (&pb.doc, row.is_some_and(|p| p.toc)) {
                    (Some(d), true) => {
                        let tree = crate::outline::heading_tree(&d.headings(), 2, 3);
                        crate::outline::to_parts(&tree, &r.url)
                    }
                    _ => Vec::new(),
                };

                // The section tree this row carries, if a `.section` unit
                // encloses it (§6e).
                let section: Vec<parts::PartMap> = row
                    .and_then(|p| crate::outline::nearest(&db.sections, &p.rel))
                    .map(|sec| {
                        let tree = section_trees.entry(sec.to_path_buf()).or_insert_with(|| {
                            crate::outline::section_tree(db, sec, &cfg.i18n.default)
                        });
                        crate::outline::to_parts(tree, &r.url)
                    })
                    .unwrap_or_default();

                // The hero (q23): an image-typed schema field, thumbnailed,
                // dimension facts attached.
                let hero = row.and_then(|p| p.hero_source()).map(|s| {
                    let t = thumbs.get(s);
                    let full = asset_url(&cfg.site.baseurl, s);
                    parts::preview(parts::Preview {
                        title: Some(title.clone()),
                        url: Some(full.clone()),
                        src: Some(t.map(|t| t.url.clone()).unwrap_or(full)),
                        dims: t.and_then(|t| t.dims),
                        ..Default::default()
                    })
                });

                // The legacy `layout:` field selects a layout kind; the
                // row's `theme:` (front matter or rule default) selects the
                // theme — per row, §5a — with a colon suffix carrying
                // subtheme tokens for CSS subselection (`recipes:spicy` →
                // data-subtheme="spicy" wherever the shell places it).
                let (theme_name, subtheme) = match row.and_then(|p| p.theme.as_deref()) {
                    Some(spec) => {
                        let (n, s) = theme::split_spec(spec);
                        (Some(n), s)
                    }
                    None => (None, None),
                };
                let row_thm = themes.get(theme_name)?;
                let row_css = css_of(theme_name);
                let head =
                    render::head_simple(&title, &r.url, &site, row.is_some_and(|p| p.noindex));
                // §5g/q44: the row picks its shell. `none` is the whole
                // point of the field — the body IS the output, so an
                // imported document can carry front matter (title, tags,
                // hidden) without being nested inside a second `<html>`.
                // Absent, the legacy `layout:` still chooses (q33(f)).
                let shell = row.and_then(|p| p.shell.as_deref());
                if shell == Some("none") {
                    out_map.insert(r.url.clone(), frag.clone().into_bytes());
                    stats.pages += 1;
                    continue;
                }
                let tier = match shell {
                    Some("light") => Theme::Light,
                    Some("html") => Theme::Default,
                    _ => Theme::parse(layout),
                };
                // §6f: engine vocabulary and the shell's `lang` resolve
                // per row locale, in both tiers.
                let row_locale = row
                    .map(|p| p.locale.as_str())
                    .unwrap_or(cfg.i18n.default.as_str());
                let html = match tier {
                    // A TIER, not a theme (§5g "Row tiers"): the minimal
                    // head (title + robots) in the same root shell as
                    // everything, around the canonical rendering. It
                    // bypasses the theme registry rather than selecting
                    // the null theme, which takes the full head.
                    Theme::Light => render::root_shell(
                        &render::light_head(&head),
                        row_locale,
                        None,
                        profile,
                        &parts::canonical(&parts::raw(frag)),
                    ),
                    Theme::Default => {
                        let bl = backlinks.get(&r.url).map(Vec::as_slice).unwrap_or(&[]);
                        // §6f: this page in other locales.
                        let translations: Vec<(String, String)> = row
                            .and_then(|p| {
                                db.by_logical.get(&p.logical).map(|sibs| {
                                    sibs.iter()
                                        .filter_map(|k| db.rows.get(k))
                                        .filter(|s| s.url != p.url)
                                        .map(|s| (s.locale.clone(), s.url.clone()))
                                        .collect()
                                })
                            })
                            .unwrap_or_default();
                        let mut head = head;
                        head.alternates =
                            locale_alternates(&cfg.site.url, row_locale, &r.url, &translations);
                        let main = match layout {
                            Some("page") | Some("post") => {
                                let mut doc = parts::document_tree(
                                    cfg,
                                    row_locale,
                                    &crate::trails::home_url(cfg, db, row_locale),
                                    &title,
                                    &r.url,
                                    parts::TreeDoc {
                                        ancestors: &crate::trails::ancestors(cfg, db, &r.url),
                                        section,
                                        outline,
                                        hero,
                                        backlinks: bl,
                                        translations: &translations,
                                    },
                                    frag,
                                );
                                if let Some(row) = row {
                                    parts::fill_from_fields(
                                        &mut doc,
                                        row,
                                        &schemas,
                                        &resolve_asset,
                                    )?;
                                }
                                row_thm.fragments.render(&doc)
                            }
                            // `default`, `null`: the row builds its own `main`.
                            _ => frag.clone(),
                        };
                        let dir = src.parent().unwrap_or(&root);
                        row_thm.page(
                            render::head_html(&head, &row_css),
                            &cfg.site.title,
                            main,
                            dir,
                            row_locale,
                            &fill_link_resolver(cfg, &linkspace, row_locale),
                            subtheme.as_deref(),
                            profile,
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
    css_pass(&theme_dir, "/css/main.css", &mut out_map, &mut stats)?;
    for name in themes.names().filter(|n| *n != "default") {
        css_pass(
            &root.join("themes").join(name),
            &format!("/css/{name}.css"),
            &mut out_map,
            &mut stats,
        )?;
    }

    stats.on_demand = materialize_referenced(db, &mut out_map, &cfg.site.url)?;

    Ok((out_map, stats))
}

/// Thumbnails (§6b): derive images once, publish under `/static/`, and hand
/// back `{% image %}` source → published URL for the render passes to look
/// up. Sources come from post bodies and rendered page bodies alike
/// (`code/legacy/*` pages use the tag too). The cache is content-addressed,
/// so a warm build only reads and hashes each source; a cold one decodes,
/// resizes and re-encodes.
fn thumbs_pass(
    cfg: &Config,
    db: &SiteDb,
    root: &Path,
    out_map: &mut SiteOutput,
    stats: &mut Stats,
) -> Result<HashMap<String, crate::thumbs::Thumb>> {
    let mut img_sources: Vec<String> = Vec::new();
    for p in db.posts() {
        img_sources.extend(tags::image_sources(&crate::store::read_body(&p.path)?));
    }
    // Image-typed schema fields (§5b) — covers and the like — thumbnail
    // too: they are what heroes and cards render (q23).
    for p in db.pages() {
        // An absolute url names something outside the site (load.rs leaves it
        // alone for the same reason): there is no file here to thumbnail.
        img_sources.extend(p.images.values().filter(|s| !is_absolute_url(s)).cloned());
    }
    for r in &db.routes {
        if r.kind == RouteKind::Page {
            if let Some(src) = &r.source {
                if let Ok(text) = std::fs::read_to_string(src) {
                    let (_, body) = split_front_matter(&text);
                    img_sources.extend(tags::image_sources(body));
                }
            }
        }
        // Gallery members (object-backed views) thumbnail too — the gallery
        // pass shows thumbs and links originals, same as {% image %}.
        if let Some(view) = &r.view {
            if view_base_kind(cfg, view) == Some(Kind::Objects) {
                for k in &r.members {
                    if let Some(o) = db.rows.get(k) {
                        img_sources.push(o.rel.to_string_lossy().to_string());
                    }
                }
            }
        }
    }
    let cache_dir = root.join("_cache/thumbs");
    let thumbs = crate::thumbs::generate(root, &cache_dir, &cfg.site.baseurl, &img_sources)?;
    let mut published: HashSet<String> = HashSet::new();
    for t in thumbs.values() {
        if published.insert(t.rel.clone()) {
            let bytes = std::fs::read(&t.cache_path)
                .with_context(|| format!("reading thumb {}", t.cache_path.display()))?;
            out_map.insert(format!("/{}", t.rel), bytes);
            stats.thumbs += 1;
        }
    }
    Ok(thumbs)
}

/// ONE render per post (§6d). Expand + parse once; the same parse yields the
/// whole document (posts, feed) and the block sequence each listing view
/// projects its summaries from.
/// The Doc is kept whole because truncation is VIEW policy (`summary = {
/// max_blocks, max_chars }`), not a property of the body.
fn render_bodies<'a>(
    cfg: &Config,
    db: &'a SiteDb,
    thumbs: &HashMap<String, crate::thumbs::Thumb>,
    linkspace: &crate::links::LinkSpace,
) -> Result<HashMap<&'a str, Doc>> {
    let root = cfg.root();
    // Posts only: these rows hold their body in memory. Tree rows are
    // re-read at render time (§2), which `render_page_bodies` does — the
    // loader asymmetry that outlives the row-type merge.
    db.post_ix
        .par_iter()
        .filter_map(|k| db.rows.get(k))
        .map(|p| -> Result<(&str, Doc)> {
            let cx = tags::Ctx {
                thumbs: Some(thumbs),
                widgets: Some(&cfg.widgets),
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
            let doc = crate::markdown::render_doc_with(&expanded, &|href| {
                crate::links::resolve(
                    cfg,
                    linkspace,
                    &dir,
                    &p.url,
                    &p.locale,
                    &p.rel.to_string_lossy(),
                    href,
                )
            })?;
            Ok((p.url.as_str(), doc))
        })
        .collect()
}

/// A rendered page body: the expanded fragment plus its Doc (markdown
/// pages) for outline extraction. Computed BEFORE any page is themed so
/// the link graph (q38) can scan every body first.
pub(crate) struct PageBody {
    pub(crate) frag: String,
    pub(crate) doc: Option<Doc>,
    /// An unimplemented construct survived expansion; the page is skipped.
    pub(crate) skipped: bool,
}

fn render_page_bodies(
    cfg: &Config,
    db: &SiteDb,
    site: &Site,
    thm: &theme::Theme,
    thumbs: &HashMap<String, crate::thumbs::Thumb>,
    linkspace: &crate::links::LinkSpace,
) -> Result<HashMap<String, PageBody>> {
    let mut out = HashMap::new();
    for r in &db.routes {
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
        // Expand FIRST, then decide: most pages that look unsupported use
        // only constructs the expander already handles.
        let cx = tags::Ctx {
            includes: Some(cfg.root().join("_includes")),
            site: Some(site),
            thumbs: Some(thumbs),
            theme: Some(thm),
            widgets: Some(&cfg.widgets),
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
        let row = db.by_url.get(r.url.as_str()).and_then(|k| db.rows.get(k));
        let dir = row
            .map(|p| p.rel.parent().map(Path::to_path_buf).unwrap_or_default())
            .unwrap_or_default();
        let locale = row.map(|p| p.locale.as_str()).unwrap_or(&cfg.i18n.default);
        let rel = row
            .map(|p| p.rel.to_string_lossy().to_string())
            .unwrap_or_default();
        let resolve =
            |href: &str| crate::links::resolve(cfg, linkspace, &dir, &r.url, locale, &rel, href);
        let (frag, doc) = if src.extension().is_some_and(|e| e == "md") {
            let d = crate::markdown::render_doc_with(&expanded, &resolve)?;
            (d.whole.clone(), Some(d))
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
            let raw = |href: &str| {
                if embeds && linkspace.is_route(href) {
                    return Ok(None);
                }
                resolve(href)
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
fn materialize_referenced(
    db: &mut SiteDb,
    out_map: &mut SiteOutput,
    site_url: &str,
) -> Result<usize> {
    // Every row that publishes only when cited, by the URL a citation names.
    let mut pending: HashMap<String, grackle_db::Key> = db
        .rows
        .iter()
        .filter(|p| p.on_demand && !p.url.is_empty())
        .map(|p| (p.url.clone(), p.key.clone()))
        .collect();
    if pending.is_empty() {
        return Ok(0);
    }

    // Seed from everything already written. Each document is scanned against
    // its OWN url, because a citation is usually relative: `code/legacy/
    // romtool/index.html` says `<img src="screen1.png">`, which §6a records
    // as how that content has always been organised.
    let mut frontier: Vec<String> = out_map
        .iter()
        .filter_map(|(u, b)| std::str::from_utf8(b).ok().map(|t| (u.as_str(), t)))
        .flat_map(|(u, t)| cited_urls(t, u, site_url))
        .collect();

    let mut made = 0usize;
    while !frontier.is_empty() {
        let mut next: Vec<String> = Vec::new();
        for url in std::mem::take(&mut frontier) {
            let Some(key) = pending.remove(&url) else {
                continue; // already materialized, or not ours to publish
            };
            let Some(row) = db.rows.get(&key) else {
                continue;
            };
            let (path, url) = (row.path.clone(), row.url.clone());
            let bytes = std::fs::read(&path)
                .with_context(|| format!("on-demand publish: reading {}", path.display()))?;
            // A materialized text file can cite more.
            if let Ok(text) = std::str::from_utf8(&bytes) {
                next.extend(cited_urls(text, &url, site_url));
            }
            db.routes.push(Route {
                source: Some(path),
                ..Route::new(url.clone(), RouteKind::Object)
            });
            out_map.insert(url, bytes);
            made += 1;
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
mod cited_url_tests {
    use super::cited_urls;

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

fn backlinks_map(
    db: &SiteDb,
    bodies: &HashMap<&str, Doc>,
    page_bodies: &HashMap<String, PageBody>,
    site_url: &str,
) -> HashMap<String, Vec<Backlink>> {
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
        let html = bodies
            .get(p.url.as_str())
            .map(|d| d.whole.as_str())
            .or_else(|| {
                page_bodies
                    .get(&p.url)
                    .filter(|pb| !pb.skipped)
                    .map(|pb| pb.frag.as_str())
            });
        if let Some(html) = html {
            sources.push((
                p.url.as_str(),
                p.title.clone().unwrap_or_default(),
                p.date,
                html,
            ));
        }
    }

    let mut map: HashMap<String, Vec<Backlink>> = HashMap::new();
    for (src_url, title, date, html) in sources {
        let mut seen: HashSet<String> = HashSet::new();
        for t in cited_urls(html, src_url, site_url) {
            if t != src_url && is_target.contains(t.as_str()) && seen.insert(t.clone()) {
                map.entry(t)
                    .or_default()
                    .push((title.clone(), src_url.to_string(), date));
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
    map
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
    bodies: &HashMap<&str, Doc>,
    page_bodies: &HashMap<String, PageBody>,
    out_map: &mut SiteOutput,
    stats: &mut Stats,
) -> Result<()> {
    let mut any = false;
    for star in &db.routes {
        let Some(view) = &star.view else { continue };
        let Some(v) = cfg.views.get(view) else {
            continue;
        };
        if v.shell.as_deref() != Some("search") {
            continue;
        }
        let route = &star.url;
        let page_by_url: HashMap<&str, &crate::db::Row> =
            db.pages().map(|p| (p.url.as_str(), p)).collect();
        // Resolved at load, like the sitemap's.
        let docs: Vec<grackle_search_core::SearchDoc> = star
            .route_members
            .iter()
            .filter_map(|k| db.routes.get(k))
            .filter_map(|r| match r.kind {
                crate::db::RouteKind::Post => {
                    db.by_url.get(&r.url).and_then(|k| db.rows.get(k)).map(|p| {
                        grackle_search_core::SearchDoc {
                            url: p.url.clone(),
                            title: p.title.clone().unwrap_or_else(|| p.url.clone()),
                            date: p.date.map(crate::db::pretty_date).unwrap_or_default(),
                            html: bodies
                                .get(p.url.as_str())
                                .map(|d| d.whole.clone())
                                .unwrap_or_default(),
                            tags: p.tags.clone(),
                        }
                    })
                }
                crate::db::RouteKind::Page => {
                    let pb = page_bodies.get(&r.url).filter(|pb| !pb.skipped)?;
                    let p = page_by_url.get(r.url.as_str())?;
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

/// A theme owns its stylesheet (§5e) — `theme.scss` compiles to the URL the
/// theme's pages link (`default` keeps /css/main.css for parity).
fn css_pass(
    theme_dir: &Path,
    url: &str,
    out_map: &mut SiteOutput,
    stats: &mut Stats,
) -> Result<()> {
    let scss = theme_dir.join("theme.scss");
    if !scss.exists() {
        return Ok(());
    }
    let text = std::fs::read_to_string(&scss)?;
    let (_, body) = split_front_matter(&text);
    let flat = inline_imports(body, theme_dir, &mut Vec::new())?;
    let opts = grass::Options::default().load_path(theme_dir);
    match grass::from_string(flat, &opts) {
        Ok(css) => {
            stats.css += css.len();
            out_map.insert(url.to_string(), css.into_bytes());
        }
        Err(e) => eprintln!("scss: {e}"),
    }
    Ok(())
}

/// The kind of the collection at the base of a view's `over` chain — what
/// decides which render pass owns its routes. None for `over = "*"`.
fn view_base_kind(cfg: &Config, view: &str) -> Option<Kind> {
    let base = cfg.query(view).ok()?.base;
    Some(cfg.collections.get(&base)?.kind)
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
) -> impl Fn(&Path, &str) -> Result<Option<String>> + 'a {
    move |owner: &Path, href: &str| {
        crate::links::resolve(
            cfg,
            space,
            owner,
            "\u{0}",
            locale,
            &format!("{}/.slots", owner.display()),
            href,
        )
    }
}

/// Pagination for a paginated route (those carrying a page number);
/// grouped views (tags, archives) have `page: None` and get nothing.
/// q32 settled: page URLs render from the owning view's own route
/// templates (locale-prefixed like the routes were), not from a literal
/// copy in the producer.
pub(crate) fn pagination_parts(
    db: &SiteDb,
    view: &str,
    v: &View,
    r: &Route,
) -> Result<Option<parts::PartMap>> {
    let Some(cur) = r.page else { return Ok(None) };
    let total = db
        .routes
        .iter()
        .filter(|x| x.view == r.view && x.page.is_some() && x.locale == r.locale)
        .count();
    let prefix = r
        .locale
        .as_deref()
        .map(|l| format!("/{l}"))
        .unwrap_or_default();
    let urls: Vec<String> = (1..=total)
        .map(|n| -> Result<String> {
            let tmpl = if n == 1 {
                v.routes.first()
            } else {
                v.routes.get(1).or_else(|| v.routes.first())
            }
            .ok_or_else(|| anyhow::anyhow!("view {view}: no routes"))?;
            Ok(format!(
                "{prefix}{}",
                crate::template::render(tmpl, |k| match k {
                    "n" => Some(n.to_string()),
                    _ => None,
                })?
            ))
        })
        .collect::<Result<_>>()?;
    Ok(parts::pagination(cur, &urls))
}

/// q45 mode A: the view's declared intro, rendered as markdown through
/// the locale-aware link resolver — an intro may say `view:…` or link a
/// source path and gets the same strict validation as any body. Config
/// prose has no directory, so the browser-agreement bypass is disabled
/// (the impossible url_dir, as shared fills do) and every link
/// canonicalizes.
fn intro_html(
    cfg: &Config,
    v: &View,
    view: &str,
    linkspace: &crate::links::LinkSpace,
    locale: &str,
) -> Result<Option<String>> {
    let Some(i) = &v.intro else { return Ok(None) };
    render_config_prose(cfg, linkspace, locale, &format!("view {view}: intro"), i)
}

/// An object row as a preview: the row IS the picture, so it is its own
/// thumbnail source and its stem is the only label it has. `row` stays unset
/// — an object has no date, tags or prose to answer with.
pub(crate) fn object_preview<'a>(
    o: &crate::db::Row,
    thumbs: &HashMap<String, crate::thumbs::Thumb>,
) -> parts::Preview<'a> {
    let t = thumbs.get(&o.rel.to_string_lossy().to_string());
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
    thumbs: &HashMap<String, crate::thumbs::Thumb>,
    content: Option<String>,
    truncated: bool,
) -> parts::Preview<'a> {
    let t = p.hero_source().and_then(|s| thumbs.get(s));
    parts::Preview {
        row: Some(p),
        content,
        truncated,
        src: t.map(|t| t.url.clone()),
        dims: t.and_then(|t| t.dims),
        tags: parts::tag_stream(cfg, p),
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
    intro_html(cfg, v, view, linkspace, locale)
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
    let doc = crate::markdown::render_doc_with(text, &|href| {
        crate::links::resolve(cfg, linkspace, Path::new(""), "\u{0}", locale, source, href)
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
        let path = load.join(format!("_{name}.scss"));
        if !path.exists() {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        seen.push(name.to_string());
        let inner = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
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

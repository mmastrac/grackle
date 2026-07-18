//! Render the database to a set of URL → bytes outputs (DESIGN.md §7).
//!
//! `render_site` produces the whole site in memory, keyed by URL. Both clients
//! consume it: `build` writes the map to disk (AOT), and `serve` holds it
//! resident and answers requests from it — the "no output directory in dev"
//! the design calls for. One render path, two materializations.

use anyhow::{Context, Result};
use rayon::prelude::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::config::{Config, View};
use crate::db::{Post, Route, RouteKind, SiteDb};
use crate::markdown::Doc;
use crate::render::{self, Site, Theme};
use crate::parts;
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
    /// XML serializations written: the feed and the sitemap.
    pub serialized: usize,
    /// Distinct derived thumbnails published under `/static/`.
    pub thumbs: usize,
    pub skipped: Vec<String>,
    /// Posts whose embeddings are missing or stale (§6b). The caller decides
    /// when to run the model: `build` before rendering, `serve` in the
    /// background with a re-render on completion.
    pub embed_pending: Vec<crate::embed::Pending>,
    /// Size of the shipped /search.bin index.
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

/// Write a rendered site to a directory (AOT). Thin wrapper over `render_site`.
pub fn build(cfg: &Config, db: &SiteDb, out: &Path) -> Result<Stats> {
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
pub fn render_site(cfg: &Config, db: &SiteDb) -> Result<(SiteOutput, Stats)> {
    let mut out_map: SiteOutput = BTreeMap::new();

    let css_url = format!("{}/css/main.css", cfg.site.baseurl);
    let site = Site {
        url: &cfg.site.url,
        title: &cfg.site.title,
        author: &cfg.site.author,
        email: cfg.site.email.as_deref(),
        css: &css_url,
    };

    let mut stats = Stats::default();

    let root = cfg.root();
    let theme_dir = root.join("themes/default");

    let thumb_urls = thumbs_pass(cfg, db, &root, &mut out_map, &mut stats)?;

    // ---- theme: fragments + tree slot fills, loaded once (§5e). All theme
    // errors — malformed fragment, unknown slot, arity violation — surface
    // here, before anything renders.
    let thm = theme::Theme::load(&theme_dir, &root)
        .with_context(|| format!("loading {}", theme_dir.display()))?;

    let bodies = render_bodies(cfg, db, &thumb_urls)?;

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
            crate::embed::Loaded { vectors: Vec::new(), pending: Vec::new() }
        }
    };
    let related = crate::embed::rank(db, &loaded.vectors, &cfg.related);
    stats.embed_pending = loaded.pending;

    // ---- posts: document parts -> theme fragments -> shell
    let rendered: Vec<(String, String)> = db
        .posts
        .rows
        .par_iter()
        .map(|p| -> Result<(String, String)> {
            let head = render::head_for_post(p, &site);
            let trail = post_trail(cfg, p);
            let whole = bodies[p.url.as_str()].whole.as_str();
            let rel: Vec<usize> = db
                .posts
                .by_url
                .get(&p.url)
                .and_then(|i| related.by_post.get(i))
                .map(|v| v.iter().map(|(j, _)| *j).collect())
                .unwrap_or_default();
            let main = thm.fragments.render(&parts::document(db, p, whole, trail, &rel));
            let dir = p.path.parent().unwrap_or(&root);
            let html = thm.page(render::head_html(&head, &site), &cfg.site.title, main, dir)?;
            Ok((p.url.clone(), html))
        })
        .collect::<Result<Vec<_>>>()?;
    for (url, html) in rendered {
        out_map.insert(url, html.into_bytes());
        stats.posts += 1;
    }

    // ---- listing views: one layout kind, the view supplies the query
    //
    // `r.members` is `self` — the rows this route materializes, decided once by
    // the view's declared query (DESIGN.md §5c). This loop used to re-derive
    // them with a `match` on the view *name*, hardcoding each filter and the
    // page size, which is how `blog_index`'s `!draft` and the config's filter
    // could silently disagree. The renderer no longer knows what a tag is.
    for r in &db.routes {
        let Some(view) = &r.view else { continue };
        let Some(v) = cfg.views.get(view) else { continue };
        // Only the built-in listing kinds render here; feed/sitemap have their
        // own passes, and a view with no layout is embedded, not routed.
        let Some(_layout) = v.layout.as_deref().or(match view.as_str() {
            "blog_index" => Some("blog_index"),
            _ => None,
        }) else {
            continue;
        };
        // The preview is the row's computed `summary` field (§6d): a
        // derived column the view declares (or inherits along `over` — the
        // field set flows with rows through composition). The 93% that CSS
        // used to hide never leaves the build; `truncated` rides along as
        // the deriver's fact, gating the theme's ★. No summary field in the
        // chain = rows ship whole.
        let summary_field = cfg.fields_for(view).get("summary").and_then(|f| f.truncate);
        let rows: Vec<(&crate::db::Post, String, bool)> = r
            .members
            .iter()
            .map(|&i| &db.posts.rows[i])
            .map(|p| match bodies.get(p.url.as_str()) {
                Some(d) => match summary_field {
                    Some(t) => {
                        let (html, truncated) = d.truncate(t.max_blocks, t.max_chars);
                        (p, html, truncated)
                    }
                    None => (p, d.whole.clone(), false),
                },
                None => (p, String::new(), false),
            })
            .collect();

        let (title, trail) = listing_title_and_trail(cfg, view, v, r)?;

        // Pagination is emitted only for paginated routes (those carrying a
        // page number); grouped views (tags, archives) have `page: None`. The
        // total is this view's page count — general, though only `blog_index`
        // paginates today.
        let pagination = match r.page {
            Some(cur) => {
                let total = db
                    .routes
                    .iter()
                    .filter(|x| x.view == r.view && x.page.is_some())
                    .count();
                parts::pagination(cur, total)
            }
            None => None,
        };

        let main = thm.fragments.render(&parts::listing(&rows, &title, trail, pagination));
        let head = render::head_simple(&title, &r.url, &site, view != "blog_index");
        let html = thm.page(render::head_html(&head, &site), &cfg.site.title, main, &root)?;
        out_map.insert(r.url.clone(), html.into_bytes());
        stats.listings += 1;
    }

    // ---- feed: the atom.xml serialization (the `feed` view's template).
    //
    // A serialization, not a themed page — it bypasses the shell entirely (§5e:
    // "feed bypasses themes; serializations have no look"). The route already
    // carries its members (the 20 newest published, newest-first); we render
    // each body and apply the feed's content transforms.
    let updated = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S+00:00").to_string();
    for r in &db.routes {
        let Some(view) = &r.view else { continue };
        let Some(v) = cfg.views.get(view) else { continue };
        if v.template.as_deref() != Some("atom.xml") {
            continue;
        }
        let entries: Vec<(&crate::db::Post, &str)> = r
            .members
            .iter()
            .map(|&i| &db.posts.rows[i])
            .map(|p| (p, bodies.get(p.url.as_str()).map(|d| d.whole.as_str()).unwrap_or("")))
            .collect();
        let xml = render::feed(&site, &updated, &entries);
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
    // deliberately dropped. The URL *set* is identical; only 42 noise lastmods
    // are absent. (DESIGN §4a is the related draft/hidden concern.)
    for (name, v) in &cfg.views {
        if v.over != "*" {
            continue;
        }
        let Some(route_tmpl) = &v.route else { continue };
        let pred = match &v.filter {
            Some(src) => crate::filter::Filter::parse(src, &crate::db::route_schema())
                .with_context(|| format!("view {name}: filter {src:?}"))?,
            None => crate::filter::Filter::always(),
        };
        let entries: Vec<(String, Option<String>)> = db
            .routes
            .iter()
            .filter(|r| pred.eval(*r))
            .map(|r| {
                let loc = format!("{}{}", site.url, r.url);
                let lastmod = match r.kind {
                    crate::db::RouteKind::Post => db
                        .posts
                        .by_url
                        .get(&r.url)
                        .and_then(|&i| db.posts.rows[i].date)
                        .map(render::xmlschema),
                    _ => None,
                };
                (loc, lastmod)
            })
            .collect();
        let xml = render::sitemap(&entries);
        out_map.insert(route_tmpl.clone(), xml.into_bytes());
        stats.serialized += 1;
    }

    // ---- tree: rendered pages + static passthrough + objects
    for r in &db.routes {
        match r.kind {
            RouteKind::Static | RouteKind::Object => {
                let Some(src) = &r.source else { continue };
                let bytes = std::fs::read(src)
                    .with_context(|| format!("reading {}", src.display()))?;
                out_map.insert(r.url.clone(), bytes);
                stats.copied += 1;
            }
            RouteKind::Page => {
                let Some(src) = &r.source else { continue };
                // scss is compiled below, not copied.
                if src.extension().is_some_and(|e| e == "scss" || e == "sass") {
                    continue;
                }
                let row = db.pages.rows.iter().find(|p| p.url == r.url);
                let text = std::fs::read_to_string(src)?;
                let (_, body) = crate::store::split_front_matter(&text);
                let layout = row.and_then(|p| p.layout.as_deref());
                let title = row
                    .and_then(|p| p.title.clone())
                    .unwrap_or_default();

                // Expand what we know FIRST, then decide. Skipping on a bare
                // "contains {%" was wrong: 17 of the 18 skipped pages used only
                // `{% image %}` / `{% post_url %}` / `{{ site.baseurl }}`, all
                // of which the expander already handles.
                let cx = tags::Ctx {
                    includes: Some(cfg.root().join("_includes")),
                    site: Some(&site),
                    thumbs: Some(&thumb_urls),
                    theme: Some(&thm),
                    widgets: Some(&cfg.widgets),
                    ..tags::Ctx::new(db, &cfg.site.baseurl, src.display().to_string())
                };
                let expanded = tags::expand(body, &cx)?;
                if expanded.contains("{%") {
                    // A construct we do not implement survived expansion.
                    stats.skipped.push(r.url.clone());
                    continue;
                }

                let frag = if src.extension().is_some_and(|e| e == "md") {
                    crate::markdown::render(&expanded)
                } else {
                    expanded
                };

                // The legacy `layout:` field selects a theme + a layout kind.
                let head = render::head_simple(&title, &r.url, &site, false);
                let html = match Theme::parse(layout) {
                    // `light` IS the null theme (§5e step 4): the minimal
                    // shell (title + robots) around the canonical rendering.
                    Theme::Light => {
                        render::light_shell(&head, &parts::canonical(&parts::raw(&frag)))
                    }
                    Theme::Default => {
                        let main = match layout {
                            Some("page") | Some("post") => thm.fragments.render(
                                &parts::document_tree(&title, &r.url, &ancestors(db, &r.url), &frag),
                            ),
                            // `default`, `null`: the row builds its own `main`.
                            _ => frag.clone(),
                        };
                        let dir = src.parent().unwrap_or(&root);
                        thm.page(render::head_html(&head, &site), &cfg.site.title, main, dir)?
                    }
                };
                out_map.insert(r.url.clone(), html.into_bytes());
                stats.pages += 1;
            }
            _ => {}
        }
    }

    search_pass(db, &bodies, &theme_dir, &mut out_map, &mut stats);
    css_pass(&theme_dir, &mut out_map, &mut stats)?;

    Ok((out_map, stats))
}

// ------------------------------------------------------------------ passes

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
) -> Result<HashMap<String, String>> {
    let mut img_sources: Vec<String> = Vec::new();
    for p in &db.posts.rows {
        img_sources.extend(tags::image_sources(&p.body));
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
    }
    let cache_dir = root.join("_cache/thumbs");
    let thumbs = crate::thumbs::generate(root, &cache_dir, &cfg.site.baseurl, &img_sources)?;
    let mut thumb_urls: HashMap<String, String> = HashMap::new();
    let mut published: HashSet<String> = HashSet::new();
    for (src, t) in &thumbs {
        thumb_urls.insert(src.clone(), t.url.clone());
        if published.insert(t.rel.clone()) {
            let bytes = std::fs::read(&t.cache_path)
                .with_context(|| format!("reading thumb {}", t.cache_path.display()))?;
            out_map.insert(format!("/{}", t.rel), bytes);
            stats.thumbs += 1;
        }
    }
    Ok(thumb_urls)
}

/// ONE render per post (§6d). Expand + parse once; the same parse yields the
/// whole document (posts, feed — byte-identical to the old double-render)
/// and the block sequence each listing view projects its summaries from.
/// The Doc is kept whole because truncation is VIEW policy (`summary = {
/// max_blocks, max_chars }`), not a property of the body.
fn render_bodies<'a>(
    cfg: &Config,
    db: &'a SiteDb,
    thumb_urls: &HashMap<String, String>,
) -> Result<HashMap<&'a str, Doc>> {
    db.posts
        .rows
        .par_iter()
        .map(|p| -> Result<(&str, Doc)> {
            let cx = tags::Ctx {
                thumbs: Some(thumb_urls),
                widgets: Some(&cfg.widgets),
                ..tags::Ctx::new(db, &cfg.site.baseurl, p.path.display().to_string())
            };
            let expanded = tags::expand(&p.body, &cx)?;
            Ok((p.url.as_str(), crate::markdown::render_doc(&expanded)))
        })
        .collect()
}

/// The searchable projection of the posts table (§6b) — one definition for
/// both the shipped index and `grackle query search`. `html_of` supplies a
/// post's rendered body; the CLI, which runs no render pass, passes raw
/// markdown (fine for a smoke query, not byte-faithful to the shipped index).
pub fn search_docs(
    db: &SiteDb,
    html_of: impl Fn(&Post) -> String,
) -> Vec<grackle_search_core::SearchDoc> {
    db.posts
        .rows
        .iter()
        .filter(|p| !p.draft && !p.hidden)
        .map(|p| grackle_search_core::SearchDoc {
            url: p.url.clone(),
            title: p.title.clone(),
            date: p.date.map(crate::db::pretty_date).unwrap_or_default(),
            html: html_of(p),
            tags: p.tags.clone(),
        })
        .collect()
}

/// Search (§6b): the index ships as /search.bin (postcard), consumed by the
/// SAME core code compiled to wasm — /search.wasm + its /search.js loader
/// are theme assets, fetched only when the search icon is clicked.
fn search_pass(
    db: &SiteDb,
    bodies: &HashMap<&str, Doc>,
    theme_dir: &Path,
    out_map: &mut SiteOutput,
    stats: &mut Stats,
) {
    let docs = search_docs(db, |p| {
        bodies.get(p.url.as_str()).map(|d| d.whole.clone()).unwrap_or_default()
    });
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
    out_map.insert("/search.bin".to_string(), bin);
    for asset in ["search.js", "search.wasm"] {
        let p = theme_dir.join(asset);
        if let Ok(bytes) = std::fs::read(&p) {
            out_map.insert(format!("/{asset}"), bytes);
        }
    }
}

/// The theme owns its stylesheet (§5e) — `theme.scss` compiles to the same
/// /css/main.css URL the shell links.
fn css_pass(theme_dir: &Path, out_map: &mut SiteOutput, stats: &mut Stats) -> Result<()> {
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
            stats.css = css.len();
            out_map.insert("/css/main.css".to_string(), css.into_bytes());
        }
        Err(e) => eprintln!("scss: {e}"),
    }
    Ok(())
}

/// Every trail roots the same way (§5c provenance): Home, then the
/// collection's own crumb, linked to its index.
fn trail_root(cfg: &Config, collection: &str) -> Vec<(String, Option<String>)> {
    let mut t = vec![("Home".to_string(), Some("/".to_string()))];
    if let Some(col) = cfg.collections.get(collection) {
        if let (Some(c), Some(u)) = (&col.crumb, &col.index) {
            t.push((c.clone(), Some(u.clone())));
        }
    }
    t
}

/// A listing route's title and provenance trail (§5c): the view's declared
/// `title`/`crumb` templates rendered over the route's group params — each
/// grouped *ancestor* linked to its own archive, this route's crumb as the
/// inert tail. This used to be a `match` on the layout kind re-deriving what
/// the config already knew; layout kinds are code, naming is the view's.
fn listing_title_and_trail(
    cfg: &Config,
    view: &str,
    v: &View,
    r: &Route,
) -> Result<(String, Vec<(String, Option<String>)>)> {
    let param = |k: &str| crate::route::param(&r.params, k);
    let title = match &v.title {
        Some(t) => crate::route::render(t, param)
            .with_context(|| format!("view {view}: title"))?,
        None => r.key.clone().unwrap_or_else(|| view.to_string()),
    };
    let tail = match r.page {
        // Paginated trails keep the engine's `Page N` rule for now — crumb
        // templates for paginated views are punted with open question 30
        // (pagination × subdivision).
        Some(p) => (p > 1).then(|| format!("Page {p}")),
        None => {
            let tmpl = v.crumb.as_ref().or(v.title.as_ref());
            match tmpl {
                Some(t) => Some(crate::route::render(t, param)
                    .with_context(|| format!("view {view}: crumb"))?),
                None => r.key.clone(),
            }
        }
    };
    let mut trail = trail_root(cfg, &cfg.query(view)?.base);
    for anc in cfg.grouped_chain(view).iter().filter(|n| *n != view) {
        let av = &cfg.views[anc.as_str()];
        let tmpl = av.crumb.as_ref().or(av.title.as_ref());
        if let (Some(t), Some(route_t)) = (tmpl, av.route.as_deref()) {
            let label = crate::route::render(t, param)
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
fn post_trail(cfg: &Config, p: &Post) -> Vec<(String, Option<String>)> {
    let mut t = trail_root(cfg, "blog");
    if p.draft {
        t.push(("Drafts".to_string(), Some("/drafts".to_string())));
        t.push((p.title.clone(), None));
        return t;
    }
    let trail_view = cfg.collections.get("blog").and_then(|c| c.trail.as_deref());
    if let Some(trail_view) = trail_view {
        for name in cfg.grouped_chain(trail_view) {
            let Some(v) = cfg.views.get(&name) else { continue };
            let specs = cfg.group_specs(&name);
            let Ok(combos) = crate::views::key_combos(p, &specs) else { continue };
            let Some(combo) = combos.first() else { break }; // undated: no trail
            let params: Vec<(String, String)> =
                combo.iter().flat_map(|k| k.params.clone()).collect();
            let get = |k: &str| crate::route::param(&params, k);
            let tmpl = v.crumb.as_ref().or(v.title.as_ref());
            if let (Some(tm), Some(rt)) = (tmpl, v.route.as_deref()) {
                if let (Ok(label), Ok(url)) =
                    (crate::route::render(tm, get), crate::route::render(rt, get))
                {
                    t.push((label, Some(url)));
                }
            }
        }
    }
    if let Some(d) = p.date {
        t.push((d.format("%-d").to_string(), None));
    }
    t
}

/// Ancestor pages of a URL, outermost first — the tree relation from §5a.
///
/// Walks the URL upward and keeps the levels that are themselves rendered
/// pages, which is what `breadcrumb.rb` did by scanning every page for a
/// matching url. Here it is a lookup, because the tree is indexed.
fn ancestors(db: &SiteDb, url: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut cur = url.trim_end_matches('/');
    while let Some(i) = cur.rfind('/') {
        cur = &cur[..i];
        if cur.is_empty() {
            break;
        }
        let parent = format!("{cur}/");
        if let Some(p) = db.pages.rows.iter().find(|p| p.url == parent && p.rendered) {
            if let Some(t) = &p.title {
                out.push((parent, t.clone()));
            }
        }
    }
    out.reverse();
    out
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


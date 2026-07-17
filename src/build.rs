//! Render the database to a set of URL → bytes outputs (DESIGN.md §7).
//!
//! `render_site` produces the whole site in memory, keyed by URL. Both clients
//! consume it: `build` writes the map to disk (AOT), and `serve` holds it
//! resident and answers requests from it — the "no output directory in dev"
//! the design calls for. One render path, two materializations.

use anyhow::{Context, Result};
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::db::{RouteKind, SiteDb};
use crate::render::{self, Site, Theme};
use crate::{legacy, parts};
use crate::tags;

/// The rendered site, keyed by URL (`/blog/`, `/atom.xml`, `/css/main.css`,
/// `/static/{hash}.jpg`, …). A directory URL ends in `/` and, on disk, becomes
/// that directory's `index.html`.
pub type SiteOutput = BTreeMap<String, Vec<u8>>;

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
        baseurl: &cfg.site.baseurl,
        title: &cfg.site.title,
        author: &cfg.site.author,
        email: cfg.site.email.as_deref(),
        css: &css_url,
    };

    let mut stats = Stats {
        posts: 0,
        pages: 0,
        listings: 0,
        copied: 0,
        css: 0,
        serialized: 0,
        thumbs: 0,
        skipped: Vec::new(),
    };

    // ---- thumbnails: derive images once, publish under /static/ (§6b).
    //
    // A pre-pass, so the render passes below can resolve each `{% image %}`
    // source to its thumbnail URL by lookup. Sources come from post bodies and
    // rendered page bodies alike (`code/legacy/*` pages use the tag too). The
    // cache is content-addressed, so a warm build only reads and hashes each
    // source; a cold one decodes, resizes and re-encodes.
    let root = cfg.root();
    let mut img_sources: Vec<String> = Vec::new();
    for p in &db.posts.rows {
        img_sources.extend(tags::image_sources(&p.body));
    }
    for r in &db.routes {
        if r.kind == RouteKind::Page {
            if let Some(src) = &r.source {
                if let Ok(text) = std::fs::read_to_string(src) {
                    let (_, body) = split_fm(&text);
                    img_sources.extend(tags::image_sources(body));
                }
            }
        }
    }
    let cache_dir = root.join("_cache/thumbs");
    let thumbs = crate::thumbs::generate(&root, &cache_dir, &cfg.site.baseurl, &img_sources)?;
    let mut thumb_urls: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut published: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (src, t) in &thumbs {
        thumb_urls.insert(src.clone(), t.url.clone());
        if published.insert(t.rel.clone()) {
            let bytes = std::fs::read(&t.cache_path)
                .with_context(|| format!("reading thumb {}", t.cache_path.display()))?;
            out_map.insert(format!("/{}", t.rel), bytes);
            stats.thumbs += 1;
        }
    }

    // ---- posts: render -> document -> theme
    let rendered: Vec<(String, String)> = db
        .posts
        .rows
        .par_iter()
        .map(|p| -> Result<(String, String)> {
            let cx = tags::Ctx {
                thumbs: Some(&thumb_urls),
                ..tags::Ctx::new(db, &cfg.site.baseurl, p.path.display().to_string())
            };
            let expanded = tags::expand(&p.body, &cx)?;
            let frag = crate::markdown::render(&expanded);
            let head = render::head_for_post(p, &site);
            let main = legacy::compose(&parts::document(db, p, &frag), &site);
            let html = Theme::Default.shell(&head, &main, &site, "");
            Ok((p.url.clone(), html))
        })
        .collect::<Result<Vec<_>>>()?;
    for (url, html) in rendered {
        out_map.insert(url, html.into_bytes());
        stats.posts += 1;
    }

    // Body fragments, reused by every listing that includes this post.
    let frags: std::collections::HashMap<&str, String> = db
        .posts
        .rows
        .par_iter()
        .map(|p| {
            let cx = tags::Ctx {
                thumbs: Some(&thumb_urls),
                ..tags::Ctx::new(db, &cfg.site.baseurl, p.path.display().to_string())
            };
            let e = tags::expand(&p.body, &cx).unwrap_or_else(|_| p.body.clone());
            (p.url.as_str(), crate::markdown::render(&e))
        })
        .collect();

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
        let rows: Vec<(&crate::db::Post, String)> = r
            .members
            .iter()
            .map(|&i| &db.posts.rows[i])
            .map(|p| (p, frags.get(p.url.as_str()).cloned().unwrap_or_default()))
            .collect();

        // Titles and crumb contributions come from the view's declared
        // `title`/`crumb` templates, rendered over the route's group params
        // (§5c) — this used to be a `match` on the layout kind re-deriving
        // what the config already knew. Layout kinds are code; naming is the
        // view's.
        let param = |k: &str| r.params.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        let title = match &v.title {
            Some(t) => crate::route::render(t, param)
                .with_context(|| format!("view {view}: title"))?,
            None => r.key.clone().unwrap_or_else(|| view.clone()),
        };
        let tail = match r.page {
            // Paginated trails keep the engine's `Page N` rule for now —
            // crumb templates for paginated views are punted with open
            // question 30 (pagination × subdivision).
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

        let main = legacy::compose(&parts::listing(&rows, &title, tail.as_deref(), pagination), &site);
        let head = render::head_simple(&title, &r.url, &site, view != "blog_index");
        let html = Theme::Default.shell(&head, &main, &site, " class=\"multipost\"");
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
            .map(|p| (p, frags.get(p.url.as_str()).map(String::as_str).unwrap_or("")))
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
                let (_, body) = split_fm(&text);
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
                let theme = Theme::parse(layout);
                let main = match layout {
                    Some("page") | Some("post") => legacy::compose(
                        &parts::document_tree(&title, &r.url, &ancestors(db, &r.url), &frag),
                        &site,
                    ),
                    // `default`, `light`, `null`: the row builds its own `main`.
                    _ => legacy::compose(&parts::raw(&frag), &site),
                };
                let head = render::head_simple(&title, &r.url, &site, false);
                let html = theme.shell(&head, &main, &site, "");
                out_map.insert(r.url.clone(), html.into_bytes());
                stats.pages += 1;
            }
            _ => {}
        }
    }

    // ---- css: the default theme's stylesheet
    let scss = cfg.root().join("css/main.scss");
    if scss.exists() {
        let text = std::fs::read_to_string(&scss)?;
        let (_, body) = split_fm(&text);
        let sass_dir = cfg.root().join("_sass");
        let flat = inline_imports(body, &sass_dir, &mut Vec::new())?;
        let opts = grass::Options::default().load_path(&sass_dir);
        match grass::from_string(flat, &opts) {
            Ok(css) => {
                stats.css = css.len();
                out_map.insert("/css/main.css".to_string(), css.into_bytes());
            }
            Err(e) => eprintln!("scss: {e}"),
        }
    }

    Ok((out_map, stats))
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

fn split_fm(text: &str) -> (&str, &str) {
    let Some(rest) = text.strip_prefix("---") else {
        return ("", text);
    };
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    let mut off = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches(['\n', '\r']) == "---" {
            return (&rest[..off], &rest[off + line.len()..]);
        }
        off += line.len();
    }
    ("", text)
}

fn fm_get<'a>(fm: &'a str, key: &str) -> Option<&'a str> {
    fm.lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))
        .map(|v| v.trim())
}

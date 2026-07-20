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
use crate::db::{Post, Route, RouteKind, SiteDb};
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

    /// Every version lists every version, itself first. Omitting self is
    /// the classic hreflang mistake.
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
    let thumb_urls: HashMap<String, String> = thumbs
        .iter()
        .map(|(k, t)| (k.clone(), t.url.clone()))
        .collect();

    // ---- themes: every directory under themes/, loaded once (§5e). All
    // theme errors — malformed fragment, unknown slot, arity violation —
    // surface here, before anything renders. Theme is chosen per ROW (§5a);
    // everything not a tree page renders through the default.
    let themes = theme::Themes::load_all(&root.join("themes"), &root).context("loading themes")?;
    let thm = themes.get(None)?;

    // §6a row/view links: the resolution space, once per build.
    let linkspace = crate::links::LinkSpace::new(cfg, db, &root);
    let bodies = render_bodies(cfg, db, &thumb_urls, &linkspace)?;
    let page_bodies = render_page_bodies(cfg, db, &site, thm, &thumb_urls, &linkspace)?;

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
    let rendered: Vec<(String, String)> = db
        .posts
        .rows
        .par_iter()
        .map(|p| -> Result<(String, String)> {
            let head = render::head_for_post(p, &site);
            let trail = crate::trails::post_trail(cfg, db, p);
            let whole = bodies[p.url.as_str()].whole.as_str();
            let rel: Vec<usize> = db
                .posts
                .by_url
                .get(&p.url)
                .and_then(|i| related.by_post.get(i))
                .map(|v| v.iter().map(|(j, _)| *j).collect())
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
                .posts
                .by_logical
                .get(&p.logical)
                .map(|sibs| {
                    sibs.iter()
                        .map(|&j| &db.posts.rows[j])
                        .filter(|s| s.url != p.url)
                        .map(|s| (s.locale.clone(), s.url.clone()))
                        .collect()
                })
                .unwrap_or_default();
            let mut head = head;
            head.alternates = locale_alternates(&cfg.site.url, &p.locale, &p.url, &translations);
            let main = thm.fragments.render(&parts::document(
                cfg,
                db,
                p,
                whole,
                trail,
                &rel,
                bl,
                outline,
                &translations,
            ));
            let dir = p.path.parent().unwrap_or(&root);
            let html = thm.page(
                render::head_html(&head, &css_of(None)),
                &cfg.site.title,
                main,
                dir,
                &p.locale,
                &fill_link_resolver(cfg, &linkspace, &p.locale),
                None,
                profile,
            )?;
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
        let Some(v) = cfg.views.get(view) else {
            continue;
        };
        // q45: a view that claims content renders in the landing pass —
        // the row owns the arrangement there.
        if v.content.is_some() {
            continue;
        }
        // Listings are posts-backed; object-backed views render in the
        // gallery pass below (their `members` index a different table).
        if view_base_kind(cfg, view) != Some(Kind::Posts) {
            continue;
        }
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

        let (title, trail) = crate::trails::listing_title_and_trail(cfg, db, view, v, r)?;
        let pagination = pagination_parts(db, view, v, r)?;
        let loc = r.locale.as_deref().unwrap_or(&cfg.i18n.default);
        let intro = route_intro(cfg, v, view, r, &linkspace, loc)?;

        let main = thm.fragments.render_with(
            &parts::listing(cfg, &rows, &title, trail, intro, pagination),
            v.variant.as_deref(),
        );
        let head = render::head_simple(&title, &r.url, &site, view != "blog_index");
        let html = thm.page(
            render::head_html(&head, &css_of(None)),
            &cfg.site.title,
            main,
            &root,
            loc,
            &fill_link_resolver(cfg, &linkspace, loc),
            None,
            profile,
        )?;
        out_map.insert(r.url.clone(), html.into_bytes());
        stats.listings += 1;
    }

    // ---- galleries: object-backed views (§5 audit). The view supplied the
    // query (`match` glob + filter + order_by); this pass only shapes rows
    // into `figure` parts — thumbnail src from §6b, dimension facts from
    // the thumb pass (q26) so the browser reserves space and masonry never
    // shifts.
    for r in &db.routes {
        let Some(view) = &r.view else { continue };
        if view_base_kind(cfg, view) != Some(Kind::Objects) {
            continue;
        }
        let Some(v) = cfg.views.get(view) else {
            continue;
        };
        if v.content.is_some() {
            continue; // q45: renders in the landing pass
        }
        let (title, trail) = crate::trails::listing_title_and_trail(cfg, db, view, v, r)?;
        let items: Vec<parts::Figure> = r
            .members
            .iter()
            .map(|&i| &db.objects.rows[i])
            .map(|o| {
                let key = o.rel.to_string_lossy().to_string();
                let t = thumbs.get(&key);
                parts::Figure {
                    url: o.url.clone(),
                    src: t.map(|t| t.url.clone()).unwrap_or_else(|| o.url.clone()),
                    dims: t.and_then(|t| t.dims),
                    alt: o
                        .rel
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default(),
                }
            })
            .collect();
        let loc = r.locale.as_deref().unwrap_or(&cfg.i18n.default);
        let intro = route_intro(cfg, v, view, r, &linkspace, loc)?;
        let main = thm.fragments.render_with(
            &parts::gallery(&items, &title, trail, intro),
            v.variant.as_deref(),
        );
        let head = render::head_simple(&title, &r.url, &site, false);
        let html = thm.page(
            render::head_html(&head, &css_of(None)),
            &cfg.site.title,
            main,
            &root,
            loc,
            &fill_link_resolver(cfg, &linkspace, loc),
            None,
            profile,
        )?;
        out_map.insert(r.url.clone(), html.into_bytes());
        stats.listings += 1;
    }

    // ---- card lists: tree-backed views (§5b rows + q23 heroes). The first
    // member is featured — the book of the month leads large, the back
    // catalogue follows as cards.
    for r in &db.routes {
        let Some(view) = &r.view else { continue };
        if view_base_kind(cfg, view) != Some(Kind::Tree) {
            continue;
        }
        let Some(v) = cfg.views.get(view) else {
            continue;
        };
        if v.content.is_some() {
            continue; // q45: renders in the landing pass
        }
        let (title, trail) = crate::trails::listing_title_and_trail(cfg, db, view, v, r)?;
        let rows: Vec<parts::CardRow> = r
            .members
            .iter()
            .map(|&i| &db.pages.rows[i])
            .map(|p| {
                let t = p.hero_source().and_then(|s| thumbs.get(s));
                parts::CardRow {
                    title: p.title.clone().unwrap_or_default(),
                    url: p.url.clone(),
                    src: t.map(|t| t.url.clone()),
                    dims: t.and_then(|t| t.dims),
                    note: p.description.clone(),
                }
            })
            .collect();
        let loc = r.locale.as_deref().unwrap_or(&cfg.i18n.default);
        let intro = route_intro(cfg, v, view, r, &linkspace, loc)?;
        // q45 theme provenance, settled by Matt's observation ("the
        // courses didn't inherit the recipe theme"): theme is a ROW
        // attribute (§5a), so a listing whose members unanimously wear
        // one theme NAME wears it too — the course archive renders
        // through the recipes theme because every row it lists does.
        // Subtheme tokens (`recipes:spicy`) are one row's dress and
        // never lift to a listing. Mixed or theme-less members keep the
        // default; posts and objects carry no theme, so only
        // tree-backed listings can inherit.
        let theme_name = {
            let mut names = r.members.iter().map(|&i| {
                db.pages.rows[i]
                    .theme
                    .as_deref()
                    .map(|s| theme::split_spec(s).0)
            });
            match names.next().flatten() {
                Some(first) if names.all(|n| n == Some(first)) => Some(first),
                _ => None,
            }
        };
        let row_thm = themes.get(theme_name)?;
        let main = row_thm.fragments.render_with(
            &parts::featured_listing(&rows, v.featured, &title, trail, intro),
            v.variant.as_deref(),
        );
        let head = render::head_simple(&title, &r.url, &site, false);
        let html = row_thm.page(
            render::head_html(&head, &css_of(theme_name)),
            &cfg.site.title,
            main,
            &root,
            loc,
            &fill_link_resolver(cfg, &linkspace, loc),
            None,
            profile,
        )?;
        out_map.insert(r.url.clone(), html.into_bytes());
        stats.listings += 1;
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
        let sibs = db
            .pages
            .by_logical
            .get(content)
            .cloned()
            .unwrap_or_default();
        let row = sibs
            .iter()
            .map(|&i| &db.pages.rows[i])
            .find(|p| p.locale == loc)
            .or_else(|| {
                sibs.iter()
                    .map(|&i| &db.pages.rows[i])
                    .find(|p| p.locale == cfg.i18n.default)
            });
        let Some(row) = row else { continue }; // existence-checked at load
        let src = &row.path;

        // This route's slice, by the view's base kind.
        let embed_parts = match view_base_kind(cfg, view) {
            Some(Kind::Posts) => {
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
                let pagination = pagination_parts(db, view, v, r)?;
                parts::listing_embed(cfg, &rows, pagination)
            }
            Some(Kind::Tree) => {
                let rows: Vec<parts::CardRow> = r
                    .members
                    .iter()
                    .map(|&i| &db.pages.rows[i])
                    .map(|p| {
                        let t = p.hero_source().and_then(|s| thumbs.get(s));
                        parts::CardRow {
                            title: p.title.clone().unwrap_or_default(),
                            url: p.url.clone(),
                            src: t.map(|t| t.url.clone()),
                            dims: t.and_then(|t| t.dims),
                            note: p.description.clone(),
                        }
                    })
                    .collect();
                parts::cards_embed(&rows, v.featured)
            }
            Some(Kind::Objects) => {
                let items: Vec<parts::Figure> = r
                    .members
                    .iter()
                    .map(|&i| &db.objects.rows[i])
                    .map(|o| {
                        let key = o.rel.to_string_lossy().to_string();
                        let t = thumbs.get(&key);
                        parts::Figure {
                            url: o.url.clone(),
                            src: t.map(|t| t.url.clone()).unwrap_or_else(|| o.url.clone()),
                            dims: t.and_then(|t| t.dims),
                            alt: o
                                .rel
                                .file_stem()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_default(),
                        }
                    })
                    .collect();
                parts::gallery_embed(&items)
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
            thumbs: Some(&thumb_urls),
            theme: Some(row_thm),
            widgets: Some(&cfg.widgets),
            embed: Some((view.as_str(), SENTINEL)),
            ..tags::Ctx::new(db, &cfg.site.baseurl, src.display().to_string())
        };
        let expanded = tags::expand(body, &cx)?;
        let frag = if src.extension().is_some_and(|e| e == "md") {
            // Body links resolve at the ROUTE's locale (the slot-fill
            // precedent: prose follows its reader), from the row's dir.
            let dir = row.rel.parent().map(Path::to_path_buf).unwrap_or_default();
            let rel = row.rel.to_string_lossy().to_string();
            let d = crate::markdown::render_doc_with(&expanded, &|href| {
                crate::links::resolve(cfg, &linkspace, &dir, &r.url, loc, &rel, href)
            })?;
            d.whole.clone()
        } else {
            expanded
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
            Some("page") | Some("post") => row_thm.fragments.render(&parts::document_tree(
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
            )),
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
        // declared, not inferred from a template filename (q33's string
        // match, retired; q44 is the full generalization).
        if v.shell.as_deref() != Some("atom") {
            continue;
        }
        let entries: Vec<(&crate::db::Post, &str)> = r
            .members
            .iter()
            .map(|&i| &db.posts.rows[i])
            .map(|p| {
                (
                    p,
                    bodies
                        .get(p.url.as_str())
                        .map(|d| d.whole.as_str())
                        .unwrap_or(""),
                )
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
    // deliberately dropped. The URL *set* is identical; only 42 noise lastmods
    // are absent. (DESIGN §4a is the related draft/hidden concern.)
    for (name, v) in &cfg.views {
        // The sitemap SHELL, likewise declared.
        if v.shell.as_deref() != Some("sitemap") {
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
            .map(|&i| {
                let p = &db.posts.rows[i];
                serde_json::json!({
                    "url": p.url,
                    "title": p.title,
                    "date": p.date.map(crate::db::iso_date),
                    "date_pretty": p.date.map(crate::db::pretty_date),
                    "tags": p.tags,
                    "html": bodies.get(p.url.as_str()).map(|d| d.whole.as_str()).unwrap_or(""),
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
                let row = db.pages.rows.iter().find(|p| p.url == r.url);
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
                    parts::figure(&parts::Figure {
                        url: format!("{}/{s}", cfg.site.baseurl),
                        src: t
                            .map(|t| t.url.clone())
                            .unwrap_or_else(|| format!("{}/{s}", cfg.site.baseurl)),
                        dims: t.and_then(|t| t.dims),
                        alt: title.clone(),
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
                                db.pages.by_logical.get(&p.logical).map(|sibs| {
                                    sibs.iter()
                                        .map(|&j| &db.pages.rows[j])
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
                                row_thm.fragments.render(&parts::document_tree(
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
                                ))
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
    // Every theme compiles its own stylesheet to its own URL.
    css_pass(&theme_dir, "/css/main.css", &mut out_map, &mut stats)?;
    for name in themes.names().filter(|n| *n != "default") {
        css_pass(
            &root.join("themes").join(name),
            &format!("/css/{name}.css"),
            &mut out_map,
            &mut stats,
        )?;
    }

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
) -> Result<HashMap<String, crate::thumbs::Thumb>> {
    let mut img_sources: Vec<String> = Vec::new();
    for p in &db.posts.rows {
        img_sources.extend(tags::image_sources(&p.body));
    }
    // Image-typed schema fields (§5b) — covers and the like — thumbnail
    // too: they are what heroes and cards render (q23).
    for p in &db.pages.rows {
        img_sources.extend(p.images.values().cloned());
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
                for &i in &r.members {
                    img_sources.push(db.objects.rows[i].rel.to_string_lossy().to_string());
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
/// whole document (posts, feed — byte-identical to the old double-render)
/// and the block sequence each listing view projects its summaries from.
/// The Doc is kept whole because truncation is VIEW policy (`summary = {
/// max_blocks, max_chars }`), not a property of the body.
fn render_bodies<'a>(
    cfg: &Config,
    db: &'a SiteDb,
    thumb_urls: &HashMap<String, String>,
    linkspace: &crate::links::LinkSpace,
) -> Result<HashMap<&'a str, Doc>> {
    let root = cfg.root();
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
/// the link graph (q38) can scan every body first — this is also what
/// untangled the tree pass, which now only themes.
struct PageBody {
    frag: String,
    doc: Option<Doc>,
    /// An unimplemented construct survived expansion; the page is skipped.
    skipped: bool,
}

fn render_page_bodies(
    cfg: &Config,
    db: &SiteDb,
    site: &Site,
    thm: &theme::Theme,
    thumb_urls: &HashMap<String, String>,
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
        // Expand what we know FIRST, then decide. Skipping on a bare
        // "contains {%" was wrong: 17 of the 18 skipped pages used only
        // constructs the expander already handles.
        let cx = tags::Ctx {
            includes: Some(cfg.root().join("_includes")),
            site: Some(site),
            thumbs: Some(thumb_urls),
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
        let (frag, doc) = if src.extension().is_some_and(|e| e == "md") {
            // §6a row/view links, same as post bodies. Raw-HTML pages are
            // exempt v1 — the lol_html rewrite stage (§6d) is their seam.
            let row = db.pages.rows.iter().find(|p| p.url == r.url);
            let dir = row
                .map(|p| p.rel.parent().map(Path::to_path_buf).unwrap_or_default())
                .unwrap_or_default();
            let locale = row.map(|p| p.locale.as_str()).unwrap_or(&cfg.i18n.default);
            let rel = row
                .map(|p| p.rel.to_string_lossy().to_string())
                .unwrap_or_default();
            let d = crate::markdown::render_doc_with(&expanded, &|href| {
                crate::links::resolve(cfg, linkspace, &dir, &r.url, locale, &rel, href)
            })?;
            (d.whole.clone(), Some(d))
        } else {
            (expanded, None)
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

/// Root-relative internal link targets in a rendered fragment (q38):
/// `href` values that are root-relative or under the site's own origin,
/// fragment and query stripped.
fn internal_links(html: &str, site_url: &str) -> Vec<String> {
    let mut out = Vec::new();
    for pat in ["href=\"", "href='"] {
        let quote = pat.chars().last().unwrap();
        let mut rest = html;
        while let Some(i) = rest.find(pat) {
            let after = &rest[i + pat.len()..];
            let Some(end) = after.find(quote) else { break };
            let href = &after[..end];
            let target = if let Some(t) = href.strip_prefix(site_url) {
                Some(t)
            } else if href.starts_with('/') && !href.starts_with("//") {
                Some(href)
            } else {
                None
            };
            if let Some(t) = target {
                let t = t.split(['#', '?']).next().unwrap_or("");
                if !t.is_empty() {
                    out.push(t.to_string());
                }
            }
            rest = &after[end..];
        }
    }
    out
}

/// The reverse link graph (q38): target url → `(source title, source
/// url)`, deduped per source, sorted by title. Sources are every rendered
/// body — posts and pages alike; targets are document rows only. Reads
/// the same bytes that ship, so link and index cannot desync.
/// `url -> [(title, url, date)]`. The citing row's date rides along: a
/// backlink's source is usually a post and it has one, so an axis that
/// dropped it was throwing away *when* the citation happened — the one
/// fact that makes a backlink list readable in date order.
type Backlink = (String, String, Option<chrono::NaiveDate>);

fn backlinks_map(
    db: &SiteDb,
    bodies: &HashMap<&str, Doc>,
    page_bodies: &HashMap<String, PageBody>,
    site_url: &str,
) -> HashMap<String, Vec<Backlink>> {
    let mut is_target: HashSet<&str> = db.posts.rows.iter().map(|p| p.url.as_str()).collect();
    is_target.extend(
        db.pages
            .rows
            .iter()
            .filter(|p| p.rendered)
            .map(|p| p.url.as_str()),
    );

    // A page has no date, so the axis is legitimately mixed — which is why
    // the theme lets an undated item span rather than assuming every
    // neighbour wears a date column.
    let mut sources: Vec<(&str, String, Option<chrono::NaiveDate>, &str)> = Vec::new();
    for p in &db.posts.rows {
        if let Some(d) = bodies.get(p.url.as_str()) {
            sources.push((p.url.as_str(), p.title.clone(), p.date, d.whole.as_str()));
        }
    }
    for p in db.pages.rows.iter().filter(|p| p.rendered) {
        if let Some(pb) = page_bodies.get(&p.url) {
            if !pb.skipped {
                sources.push((
                    p.url.as_str(),
                    p.title.clone().unwrap_or_default(),
                    None,
                    pb.frag.as_str(),
                ));
            }
        }
    }

    let mut map: HashMap<String, Vec<Backlink>> = HashMap::new();
    for (src_url, title, date, html) in sources {
        let mut seen: HashSet<String> = HashSet::new();
        for t in internal_links(html, site_url) {
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

#[cfg(test)]
mod link_tests {
    use super::internal_links;

    #[test]
    fn extracts_internal_links_only() {
        let html = r##"<a href="/blog/x/">x</a> <a href='/a.png'>i</a>
            <a href="https://grack.com/blog/y/#frag">abs</a>
            <a href="https://elsewhere.com/z">ext</a>
            <a href="//cdn.example/w">proto-rel</a> <a href="#top">frag</a>"##;
        let mut links = internal_links(html, "https://grack.com");
        links.sort();
        assert_eq!(links, vec!["/a.png", "/blog/x/", "/blog/y/"]);
    }
}

/// The searchable projection of the posts table — the CLI smoke query
/// (`grackle query search`), which runs no render pass and feeds raw
/// markdown. The SHIPPED index is not this: it is the `shell = "search"`
/// view's serialization (see `search_pass`), which may span tables.
pub fn search_docs(
    db: &SiteDb,
    html_of: impl Fn(&Post) -> String,
) -> Vec<grackle_search_core::SearchDoc> {
    db.posts
        .rows
        .iter()
        .map(|p| grackle_search_core::SearchDoc {
            url: p.url.clone(),
            title: p.title.clone(),
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
    for (name, v) in &cfg.views {
        if v.shell.as_deref() != Some("search") {
            continue;
        }
        let Some(route) = &v.route else { continue };
        let pred = match &v.filter {
            Some(src) => crate::filter::Filter::parse(src, &crate::db::route_schema())
                .with_context(|| format!("view {name}: filter {src:?}"))?,
            None => crate::filter::Filter::always(),
        };
        let page_by_url: HashMap<&str, &crate::db::Page> =
            db.pages.rows.iter().map(|p| (p.url.as_str(), p)).collect();
        let docs: Vec<grackle_search_core::SearchDoc> = db
            .routes
            .iter()
            .filter(|r| pred.eval(*r))
            .filter_map(|r| match r.kind {
                crate::db::RouteKind::Post => db
                    .posts
                    .by_url
                    .get(&r.url)
                    .map(|&i| &db.posts.rows[i])
                    .map(|p| grackle_search_core::SearchDoc {
                        url: p.url.clone(),
                        title: p.title.clone(),
                        date: p.date.map(crate::db::pretty_date).unwrap_or_default(),
                        html: bodies
                            .get(p.url.as_str())
                            .map(|d| d.whole.clone())
                            .unwrap_or_default(),
                        tags: p.tags.clone(),
                    }),
                crate::db::RouteKind::Page => {
                    let pb = page_bodies.get(&r.url).filter(|pb| !pb.skipped)?;
                    let p = page_by_url.get(r.url.as_str())?;
                    Some(grackle_search_core::SearchDoc {
                        url: p.url.clone(),
                        // A titleless page is still searchable by body; its
                        // URL is the only honest label a hit can wear.
                        title: p.title.clone().unwrap_or_else(|| p.url.clone()),
                        date: String::new(),
                        // Markdown pages searched from the same bytes that
                        // ship; raw-HTML pages from their body fragment.
                        html: pb
                            .doc
                            .as_ref()
                            .map(|d| d.whole.clone())
                            .unwrap_or_else(|| pb.frag.clone()),
                        tags: Vec::new(),
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
fn fill_link_resolver<'a>(
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
fn pagination_parts(
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
                crate::route::render(tmpl, |k| match k {
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

/// The intro for one ROUTE (§6f enum records × q45 mode A): a grouped
/// route whose leaf value declares a record `intro` gets that value's
/// own prose — the course archive introduces the course — else the
/// view's intro applies to every partition.
fn route_intro(
    cfg: &Config,
    v: &View,
    view: &str,
    r: &Route,
    linkspace: &crate::links::LinkSpace,
    locale: &str,
) -> Result<Option<String>> {
    if r.key.is_some() {
        let chain = cfg.group_specs(view);
        if let Some(field) = chain.last().map(|s| crate::views::spec_field(s)) {
            if let Some(id) = crate::route::param(&r.params, field) {
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

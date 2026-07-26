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
    /// Stylesheets that failed to compile. Collected rather than thrown so
    /// the two callers can disagree: `serve` prints and keeps the loop
    /// alive, `build` refuses to publish. A site whose CSS silently failed
    /// looks deployable and is wrong, which is the one outcome worth
    /// failing a build over.
    pub css_errors: Vec<String>,
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
        assert_eq!(alt_media_type("/notes/one.md"), Some("text/markdown".into()));
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
        noindex: cfg.site.noindex,
        icon: &icon,
    };
    let profile = cfg.profile.as_deref();
    // `[html.head.meta]` (§4e), compiled once against both surfaces.
    let metas = render::compile_metas(cfg, &db.declared)?;
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
        theme::Themes::load_all(&root.join("themes"), &root, &schemas, cfg.site.theme.as_deref())
            .context("loading themes")?;
    // A view's `theme` is checked here rather than in config, because the
    // registry is what knows which names exist — the same reason `[site] theme`
    // is checked in `load_all`. A typo would otherwise fall through to the
    // default and render the wrong look silently.
    for (name, v) in &cfg.views {
        if let Some(spec) = v.theme.as_deref() {
            themes
                .get(Some(theme::split_spec(spec).0))
                .with_context(|| format!("view {name}: theme = {spec:?}"))?;
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
    let post_routes: Vec<&Route> = db
        .routes
        .iter()
        .filter(|r| r.kind == RouteKind::Post)
        .collect();
    let rendered: Vec<(String, String)> = post_routes
        .par_iter()
        .filter_map(|r| {
            r.row
                .as_ref()
                .and_then(|k| db.rows.get(k))
                .map(|p| (*r, p))
        })
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
            // §6e heading axis: `toc:` rows carry their outline, extracted
            // from the same rendered bytes. h2–h3 is the v1 depth window
            // (production policy, not CSS — never ship what a theme hides).
            let outline = if p.toc {
                let tree = crate::outline::heading_tree(&bodies[&p.key].headings(), 2, 3);
                crate::outline::to_parts(&tree, &p.url)
            } else {
                Vec::new()
            };
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
            // q53: this row's OTHER axis forms (themes, the md twin) as
            // `rel="alternate"`, beside the locale twins above.
            head.alternates
                .extend(axis_alternates(db, &cfg.site.url, r));
            let groups =
                parts::relation_groups(rel_groups.get(&p.url).cloned().unwrap_or_default());
            let mut doc = parts::document(cfg, p, whole, trail, groups, outline, &translations);
            parts::fill_from_fields(&mut doc, p, &schemas, &resolve_asset)?;
            let dir = p.path.parent().unwrap_or(&root);
            // Theme is per ROW (§5a), posts included — which means the row's
            // theme arranges the BODY too, not just the shell around it.
            // Rendering `main` through the site default here (as this did)
            // shipped one theme's chrome and stylesheet wrapped around
            // another's markup: a themed post came out as canonical fallback
            // in a themed shell, and every selector the theme wrote missed.
            let (theme_name, subtheme) = themes.resolve(axis_field(r, "theme").or(p.theme.as_deref()));
            let row_thm = themes.get(theme_name)?;
            let main = row_thm.fragments.render(&doc);
            let html = row_thm.page(
                render::head_html(&head, &css_of(theme_name)),
                &cfg.site.title,
                main,
                dir,
                &p.locale,
                &fill_link_resolver(cfg, &linkspace, &p.locale),
                subtheme.as_deref(),
                profile,
                &r.axis,
            )?;
            Ok((url.to_string(), html))
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
                        let (html, truncated) = match bodies.get(&p.key) {
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
        // landing wears its section's clothes) — unless the VIEW named one,
        // which is nearer and explicit. Same order the listing pass uses, and
        // it is what lets several routes over one query wear several looks
        // without several copies of the rows underneath them.
        // q53 first, then the view's own, then the claimed row's (§5h).
        let (theme_name, subtheme) = match axis_field(r, "theme").or(v.theme.as_deref()) {
            Some(spec) => themes.resolve(Some(spec)),
            None => themes.resolve(row.theme.as_deref()),
        };
        let row_thm = themes.get(theme_name)?;
        let embed_html = row_thm
            .fragments
            .render_with(&embed_parts, v.variant.as_deref());

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
        let groups = parts::relation_groups(rel_groups.get(&r.url).cloned().unwrap_or_default());

        // Absent `layout:` means a document (§4d). It used to mean the raw
        // body, so a row that forgot the key lost its furniture with no error
        // — which is exactly why every site had to write
        // `defaults = { layout = "post" }`. `layout: default` is the escape
        // hatch, and it is the one that always said what it meant.
        let main = match row.layout.as_deref() {
            Some("page") | Some("post") | None => {
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
                        relation_groups: groups,
                        translations: &translations,
                    },
                    &frag,
                );
                parts::fill_from_fields(&mut doc, row, &schemas, &resolve_asset)?;
                row_thm.fragments.render(&doc)
            }
            _ => frag.clone(),
        };
        let mut head = render::head_simple(&title, &r.url, &site);
        head.meta = render::eval_metas(&metas, r, &site, &title, &r.url);
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
            &r.axis,
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
                    .get(&p.key)
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
                        "html": bodies.get(&p.key).map(|d| d.whole.as_str()).unwrap_or(""),
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
                let row = r.row.as_ref().and_then(|k| db.rows.get(k));
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
                // q53: an axis member's theme beats the row's, same as the
                // post path — the member IS the alternative form.
                let (theme_name, subtheme) = themes
                    .resolve(axis_field(r, "theme").or(row.and_then(|p| p.theme.as_deref())));
                let row_thm = themes.get(theme_name)?;
                let row_css = css_of(theme_name);
                let mut head = render::head_simple(&title, &r.url, &site);
                // A page's metas read its ROW when it has one; a sourceless
                // route falls back to the route's own fields.
                head.meta = match row {
                    // The head describes the DOCUMENT, whose address is its
                    // canonical URL — the row's own, which is exactly what the
                    // canonical axis member is published at. So an alternate
                    // canonicalizes to the canonical form instead of itself,
                    // which is the difference between an alternative form and a
                    // duplicate page. Identical to `r.url` off an axis.
                    Some(p) => render::eval_metas(&metas, p, &site, &title, &p.url),
                    None => render::eval_metas(&metas, r, &site, &title, &r.url),
                };
                // §5g/q44: the row picks its shell. `none` is the whole
                // point of the field — the body IS the output, so an
                // imported document can carry front matter (title, tags,
                // hidden) without being nested inside a second `<html>`.
                // Absent, the legacy `layout:` still chooses (q33(f)).
                // q53: an axis member over `shell` is the md twin's shape — the
                // same row serialized two ways, at two URLs. The member's value
                // beats the row's own for the same reason a member's theme
                // does: the member IS the alternative form.
                let shell = axis_field(r, "shell").or(row.and_then(|p| p.shell.as_deref()));
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
                        &r.axis,
                        &parts::canonical(&parts::raw(frag)),
                    ),
                    Theme::Default => {
                        let groups = parts::relation_groups(
                            rel_groups.get(&r.url).cloned().unwrap_or_default(),
                        );
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
                        // q53: this page's other axis forms, beside the locale twins.
                        head.alternates
                            .extend(axis_alternates(db, &cfg.site.url, r));
                        // Absent means a document, per §4d — see the post arm.
                        let main = match layout {
                            Some("page") | Some("post") | None => {
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
                                        relation_groups: groups,
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
                            &r.axis,
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
    let overlay = site_overlay(&root, &mut stats);
    css_pass(
        &theme_dir,
        "/css/main.css",
        overlay.as_deref(),
        &mut out_map,
        &mut stats,
    )?;
    for name in themes.names().filter(|n| *n != "default") {
        css_pass(
            &root.join("themes").join(name),
            &format!("/css/{name}.css"),
            overlay.as_deref(),
            &mut out_map,
            &mut stats,
        )?;
    }

    stats.on_demand = materialize_referenced(db, &mut out_map, &cfg.site.url)?;

    // The §6g splice markers have done their two jobs — fencing the citation
    // scan (backlinks) while on-demand publishing above read past them — so
    // strip them before the bytes ship. Last pass, after every scanner.
    strip_view_markers(&mut out_map);
    Ok((out_map, stats))
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
        // The row first: what the expander renders an embed WITH is the
        // row's theme, not the site default (§5a). A `{% view %}` in a
        // themed page's body arranges its rows the way that page's theme
        // says, exactly as the landing path does.
        let row = r.row.as_ref().and_then(|k| db.rows.get(k));
        let row_thm = themes
            .get(themes.resolve(axis_field(r, "theme").or(row.and_then(|p| p.theme.as_deref()))).0)?;
        // Expand FIRST, then decide: most pages that look unsupported use
        // only constructs the expander already handles.
        let cx = tags::Ctx {
            includes: Some(cfg.root().join("_includes")),
            site: Some(site),
            thumbs: Some(thumbs),
            theme: Some(row_thm),
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
        css_pass(&dir, "/css/t.css", None, &mut out, &mut stats).unwrap();
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
        let html = bodies
            .get(&p.key)
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
    for star in &db.routes {
        let Some(view) = &star.view else { continue };
        let Some(v) = cfg.views.get(view) else {
            continue;
        };
        if v.shell.as_deref() != Some("search") {
            continue;
        }
        let route = &star.url;
        // Resolved at load, like the sitemap's.
        let docs: Vec<grackle_search_core::SearchDoc> = star
            .route_members
            .iter()
            .filter_map(|k| db.routes.get(k))
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
fn css_pass(
    theme_dir: &Path,
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
    if let Some(src) = own {
        let text = std::fs::read_to_string(&src)?;
        let (_, body) = split_front_matter(&text);
        let mut seen = Vec::new();
        let flat = inline_imports(body, theme_dir, &mut seen)?;

        // A `_tokens.scss` nobody imports is the dead-file trap again, one
        // arm along: the sheet compiles, the tokens are simply never read,
        // and the only symptom is a theme that ignores its own palette.
        // Same class of bug as the tokens-only theme that shipped a sheet
        // it never opened — worth a word, not a failure, because a theme
        // may legitimately keep a partial it does not use yet.
        if tokens.exists() && !seen.iter().any(|s| s == "tokens") {
            eprintln!(
                "grackle: {} has a _tokens.scss that nothing imports — add \
                 `@import \"tokens\";` to theme.scss, or the palette is dead \
                 weight",
                theme_dir.display()
            );
        }

        let opts = grass::Options::default().load_path(theme_dir);
        match grass::from_string(flat, &opts) {
            Ok(theme_css) => css.push_str(&format!(
                "@layer theme {{\n{}\n}}\n",
                strip_charset(&theme_css)
            )),
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

/// The kind of the collection at the base of a view's `over` chain — what
/// decides which render pass owns its routes. None for `over = "*"`.
fn view_base_kind(cfg: &Config, view: &str) -> Option<Kind> {
    // A union's members share a kind (`Config::check_base`), so the first
    // answers for the whole base.
    let base = cfg.query(view).ok()?.base;
    Some(cfg.collections.get(base.first()?)?.kind)
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

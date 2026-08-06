//! Render the database to a set of URL -> bytes outputs.
//!
//! `render_site` produces the whole site in memory, keyed by URL. Both clients
//! consume it: `build` writes the map to disk (AOT), and `serve` holds it
//! resident and answers requests from it, the "no output directory in dev"
//! the design calls for. One render path, two materializations.

use anyhow::{Context, Result};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::model::SiteDb;
use crate::passes::preview;
use crate::render::{self, Site};
use crate::theme;

pub mod bodies;
pub mod emit;
pub mod postpass;
pub mod prepass;
pub mod types;

pub use crate::shells::search::search_docs;
pub(crate) use preview::asset_url;
pub(crate) use types::Backlink;
pub use types::{PageBody, SiteOutput, Stats};

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
pub fn build(cfg: &Config, db: &mut SiteDb, out: &Path) -> Result<Stats> {
    // AOT builds publish, so they wait for fresh embeddings: bring the cache
    // current first, then render once with nothing pending.
    let cache = cfg.root().join("_cache/embeddings");
    if let Ok(l) = crate::embed::load(db, cfg, &cache) {
        if !l.pending.is_empty() {
            println!("grackle: embedding {} changed posts…", l.pending.len());
            if let Err(e) = crate::embed::embed_pending(&cache, &l.pending) {
                eprintln!("grackle: embedding failed, using stale/absent vectors: {e:#}");
            }
        }
    }
    let (map, stats) = render_site(cfg, db)?;
    // A publishing build refuses a stylesheet that did not compile. `serve`
    // makes the opposite call on the same data, see `Stats::css_errors`.
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

pub fn render_site(cfg: &Config, db: &mut SiteDb) -> Result<(SiteOutput, Stats)> {
    let mut out_map: SiteOutput = BTreeMap::new();

    let icon = prepass::site_icon(cfg, db);
    let canon = cfg.pairing_canonical().unwrap_or("");
    let site = Site {
        url: &cfg.site.url,
        title: cfg.site_title(canon),
        author: &cfg.site.author,
        email: cfg.site.email.as_deref(),
        icon: &icon,
    };
    let profile = cfg.profile.as_deref();
    // `[html.head.meta]` / `[html.*.attribute]`, compiled once.
    let metas = render::compile_metas(cfg, &db.declared)?;
    let attrs = render::compile_attrs(cfg, &db.declared)?;
    let mut stats = Stats::default();

    let root = cfg.root();
    let theme_dir = root.join("themes/default");

    let thumbs = prepass::thumbs_pass(cfg, db, &root, &mut out_map, &mut stats)?;

    // An image field's published URL: the thumbnail's when
    // the pass generated one, else the original under baseurl. This is the
    // presentation `fill_from_fields` delegates so it need not know either.
    // themes: every directory under themes/, loaded once. All
    // theme errors, malformed fragment, unknown slot, arity violation,
    // surface here, before anything renders. Theme is chosen per ROW.
    // Part vocabulary is derived from fragments + declared field schemas.
    let fields = part_fields_from_cfg(cfg);
    let themes = theme::Themes::load_all(
        &root.join("themes"),
        &root,
        &fields,
        cfg.site.theme.as_deref(),
    )
    .context("loading themes")?;
    prepass::check_theme_names(cfg, db, &themes)?;
    // A `.slots/` file whose stem names no slot any loaded theme places
    // fills nothing, silently. Said here rather than in the source loader
    // because the knowledge is here, the slot names come from the themes,
    // which only exist once `load_all` has run. A warning, not an error;
    // `slots::unknown_stems` carries the reasoning. `serve` rebuilds the
    // world through this function on every change, so a fixed name stops
    // being reported on the next save (the config warnings' convention, one crate over).
    {
        let locales: Vec<&str> = match cfg.pairing_axis() {
            Some((_, a)) => a.values.iter().map(String::as_str).collect(),
            None => Vec::new(),
        };
        crate::slots::check_chrome_fills(themes.fills(), &root)?;
        // `chrome` is not an identity slot, it is the cluster override,
        // consumed at theme load, but it lives in `.slots/` like one.
        let mut known = themes.identity_slots();
        known.push("chrome");
        for w in crate::slots::unknown_stems(themes.fills(), &known, &locales) {
            eprintln!("grackle: {w}");
            db.warnings.push(w);
        }
    }

    // row/view links: the resolution space, once per build.
    let linkspace = crate::links::LinkSpace::new(cfg, db, &root);
    let bodies = bodies::render_bodies(cfg, db, &thumbs, &linkspace)?;
    let page_bodies = bodies::render_page_bodies(cfg, db, &site, &themes, &thumbs, &linkspace)?;

    // ---- the link graph: scan every rendered body once, posts and
    // pages alike, and invert. Backlinks are one more relations axis; the
    // scan reads the same bytes that ship, so link and index cannot desync.
    let (backlinks, links_to) = prepass::backlinks_map(db, &bodies, &page_bodies, &cfg.site.url);

    // related posts: cache-only load, fresh vectors where the
    // cache has them, STALE ones where a post's text changed (it keeps its
    // old embedding until reprocessed), None for never-seen posts. Whatever
    // is pending goes back to the caller via Stats: `build` embeds it
    // before rendering (published output is always fresh), `serve` embeds
    // on a background thread and re-renders on completion. Ranking policy
    // ([related]: min score, year penalty/cap) is config.
    let loaded = match crate::embed::load(db, cfg, &root.join("_cache/embeddings")) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("grackle: embeddings unavailable, skipping related posts: {e:#}");
            crate::embed::Loaded {
                keys: Vec::new(),
                vectors: Vec::new(),
                pending: Vec::new(),
            }
        }
    };
    stats.embed_pending = loaded.pending;

    // The relation engine, declared neighbour queries evaluated per row,
    // with the embedding vectors and the link graph in hand. Replaces the
    // hardcoded similar/adjacency/linked-from axes; `[related]`'s knobs are
    // now grack.com's `related` rank expression. Evaluated for every rendered
    // row up front into an owned map, so the engine's borrow of `db` ends
    // before the render passes need `&mut db` again.
    let rel_groups: HashMap<String, Vec<crate::relate::Group>> = {
        let relate = crate::relate::Engine::new(
            cfg,
            db,
            &loaded.keys,
            &loaded.vectors,
            &links_to,
            &backlinks,
        );
        db.rows
            .iter()
            .filter(|r| r.rendered)
            .map(|r| (r.url.clone(), relate.groups_for(r)))
            .collect()
    };

    // Stylesheets compile ahead of the pages that link them: in `hashed` mode
    // a sheet's URL is its content address, so the `<head>` must know it at
    // render time.
    let css_urls = postpass::stylesheets(
        cfg,
        db,
        &themes,
        &root,
        &theme_dir,
        &mut out_map,
        &mut stats,
    )?;

    emit::run(
        cfg,
        db,
        &site,
        &metas,
        &attrs,
        &themes,
        &thumbs,
        &bodies,
        &page_bodies,
        &linkspace,
        &backlinks,
        &rel_groups,
        &css_urls,
        &root,
        profile,
        &mut out_map,
        &mut stats,
    )?;

    let warnings = postpass::search(
        cfg,
        db,
        &bodies,
        &page_bodies,
        &linkspace,
        &mut out_map,
        &mut stats,
    )?;
    drop(bodies);
    drop(page_bodies);
    db.warnings.extend(warnings);
    postpass::citations(cfg, db, &thumbs, &mut out_map, &mut stats)?;

    Ok((out_map, stats))
}

/// Declared `[schema]` fields as part types for vocabulary derivation.
fn part_fields_from_cfg(cfg: &Config) -> Vec<(String, grackle_source::schema::FieldType)> {
    cfg.schema
        .decls
        .iter()
        .filter_map(|(name, val)| {
            let ty = val.get("type")?.as_str()?;
            Some((name.clone(), grackle_source::schema::FieldType::parse(ty)?))
        })
        .collect()
}

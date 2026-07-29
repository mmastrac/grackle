//! Render the database to a set of URL → bytes outputs (DESIGN.md §7).
//!
//! `render_site` produces the whole site in memory, keyed by URL. Both clients
//! consume it: `build` writes the map to disk (AOT), and `serve` holds it
//! resident and answers requests from it — the "no output directory in dev"
//! the design calls for. One render path, two materializations.

use anyhow::{Context, Result};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::model::SiteDb;
use crate::parts;
use crate::passes::preview;
use crate::render::{self, Site};
use crate::theme;

pub mod bodies;
pub mod emit;
pub mod postpass;
pub mod prepass;
pub mod types;

pub use postpass::search_docs;
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

pub fn render_site(cfg: &Config, db: &mut SiteDb) -> Result<(SiteOutput, Stats)> {
    let mut out_map: SiteOutput = BTreeMap::new();

    let icon = prepass::site_icon(cfg, db);
    let site = Site {
        url: &cfg.site.url,
        title: &cfg.site.title,
        author: &cfg.site.author,
        email: cfg.site.email.as_deref(),
        icon: &icon,
    };
    let profile = cfg.profile.as_deref();
    // `[html.head.meta]` / `[html.*.attribute]` (§4e), compiled once.
    let metas = render::compile_metas(cfg, &db.declared)?;
    let attrs = render::compile_attrs(cfg, &db.declared)?;
    let mut stats = Stats::default();

    let root = cfg.root();
    let theme_dir = root.join("themes/default");

    let thumbs = prepass::thumbs_pass(cfg, db, &root, &mut out_map, &mut stats)?;

    // An image field's published URL (§5e image parts): the thumbnail's when
    // the pass generated one, else the original under baseurl. This is the
    // presentation `fill_from_fields` delegates so it need not know either.
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
    prepass::check_theme_names(cfg, db, &themes)?;
    // C4b: a `.slots/` file whose stem names no slot any loaded theme places
    // fills nothing, silently. Said here rather than in the source loader
    // because the knowledge is here — the slot names come from the themes,
    // which only exist once `load_all` has run. A warning, not an error;
    // `slots::unknown_stems` carries the reasoning. `serve` rebuilds the
    // world through this function on every change, so a fixed name stops
    // being reported on the next save (C3's convention, one crate over).
    {
        let locales: Vec<&str> = match cfg.pairing_axis() {
            Some((_, a)) => a.values.iter().map(String::as_str).collect(),
            None => vec![cfg.i18n.default.as_str()],
        };
        for w in crate::slots::unknown_stems(themes.fills(), &themes.identity_slots(), &locales) {
            eprintln!("grackle: {w}");
            db.warnings.push(w);
        }
    }

    // §6a row/view links: the resolution space, once per build.
    let linkspace = crate::links::LinkSpace::new(cfg, db, &root);
    let bodies = bodies::render_bodies(cfg, db, &thumbs, &linkspace)?;
    let page_bodies = bodies::render_page_bodies(cfg, db, &site, &themes, &thumbs, &linkspace)?;

    // ---- the link graph (q38): scan every rendered body once — posts and
    // pages alike — and invert. Backlinks are one more relations axis; the
    // scan reads the same bytes that ship, so link and index cannot desync.
    let (backlinks, links_to) = prepass::backlinks_map(db, &bodies, &page_bodies, &cfg.site.url);

    // ---- related posts (§6b): cache-only load — fresh vectors where the
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
        &root,
        profile,
        &mut out_map,
        &mut stats,
    )?;

    let warnings = postpass::search_and_css(
        cfg,
        db,
        &bodies,
        &page_bodies,
        &themes,
        &root,
        &theme_dir,
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

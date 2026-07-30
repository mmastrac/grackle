//! Body prepass: render post and page bodies before emit.

use anyhow::{Context, Result};
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::Path;

use crate::config::Config;
use crate::markdown::Doc;
use crate::model::{RouteKind, Row, SiteDb};
use crate::passes::preview::resolve_theme;
use crate::pipeline::types::PageBody;
use crate::render::Site;
use crate::store::split_front_matter;
use crate::tags;
use crate::theme;

/// ONE render per post (§6d). Expand + parse once; the same parse yields the
/// whole document (posts, feed) and the block sequence each listing view
/// projects its summaries from.
/// The Doc is kept whole because truncation is VIEW policy (`summary = {
/// max_blocks, max_chars }`), not a property of the body.
pub(crate) fn render_bodies<'a>(
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
                cfg: Some(cfg),
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
                    &cfg.pairing_member(p),
                    &p.rel.to_string_lossy(),
                    form,
                    href,
                )
            })?;
            Ok((&p.key, doc))
        })
        .collect()
}

pub(crate) fn render_page_bodies(
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
        // different arms of `shells::search::search_pass` and the feed.
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
        let row_thm =
            themes.get(resolve_theme(themes, r, row.and_then(|p| p.theme.as_deref())).0)?;
        // Expand FIRST, then decide: most pages that look unsupported use
        // only constructs the expander already handles.
        let cx = tags::Ctx {
            includes: Some(cfg.root().join("_includes")),
            site: Some(site),
            thumbs: Some(thumbs),
            theme: Some(row_thm),
            widgets: Some(&cfg.widgets),
            cfg: Some(cfg),
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
        let locale_owned = row
            .map(|p| cfg.pairing_member(p))
            .unwrap_or_default();
        let locale = locale_owned.as_str();
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

/// Post body if held, else non-skipped page body.
pub(crate) fn row_body_html<'a>(
    p: &Row,
    bodies: &'a HashMap<&grackle_model::Key, Doc>,
    page_bodies: &'a HashMap<String, PageBody>,
) -> Option<&'a str> {
    bodies.get(&p.key).map(|d| d.whole.as_str()).or_else(|| {
        page_bodies
            .get(&p.url)
            .filter(|pb| !pb.skipped)
            .map(|pb| pb.frag.as_str())
    })
}

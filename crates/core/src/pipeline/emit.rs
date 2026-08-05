//! Route emitters: posts, landings, shells, tree pages.

use anyhow::{bail, Context, Result};
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::assemble::chain;
use crate::config::Config;
use crate::markdown::Doc;
use crate::model::{Route, RouteKind, SiteDb};
use crate::parts;
use crate::passes::preview;
use crate::pipeline::prepass;
use crate::pipeline::types::{PageBody, SiteOutput, Stats};
use crate::render::{self, Site, Theme};
use crate::store::split_front_matter;
use crate::tags;
use crate::theme;

#[allow(clippy::too_many_arguments)]
/// Emit every route into `out_map`: posts, listings, landings, shells, tree.
pub(crate) fn run(
    cfg: &Config,
    db: &SiteDb,
    site: &Site<'_>,
    metas: &render::Metas,
    attrs: &render::HtmlAttrs,
    themes: &theme::Themes,
    thumbs: &crate::thumbs::Renditions,
    bodies: &HashMap<&grackle_db::Key, Doc>,
    page_bodies: &HashMap<String, PageBody>,
    linkspace: &crate::links::LinkSpace,
    backlinks: &HashMap<String, Vec<crate::pipeline::Backlink>>,
    rel_groups: &HashMap<String, Vec<crate::relate::Group>>,
    css_urls: &crate::assets::CssUrls,
    root: &Path,
    profile: Option<&str>,
    out_map: &mut SiteOutput,
    stats: &mut Stats,
) -> Result<()> {
    let resolve_asset = |src: &str| -> String {
        crate::thumbs::default_of(thumbs, src)
            .map(|t| t.url.clone())
            .unwrap_or_else(|| preview::asset_url(&cfg.site.baseurl, src))
    };
    // Chrome-part facts, computed once per build.
    let chrome_facts = preview::ChromeFacts::of(cfg, db);
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
            let lang = cfg.pairing_member(p);
            let site = site.with_title(cfg.site_title(lang.as_str()));
            let mut head = render::head_for_post(p, &site, metas);
            // The head describes the DOCUMENT, and a document's address is its
            // canonical URL — `p.url`, which is exactly what the canonical axis
            // member is published at (an alternate is templated, the canonical
            // one is not). So an alternate's `rel="canonical"` and `og:url`
            // name the canonical form rather than themselves, which is the
            // whole difference between an alternative form and a duplicate
            // page. For a row on no axis this is the route's own URL anyway.
            head.meta = render::eval_metas(metas, p, &site, &head.title, &p.url);
            head.injected = bodies[&p.key].heads.clone();
            let trail = crate::trails::post_trail(cfg, db, p);
            let whole = bodies[&p.key].whole.as_str();
            head.alternates =
                prepass::head_alternates(metas, &site, &head.title, cfg, db, r, &cfg.media_types);
            let (theme_name, subtheme) = preview::resolve_theme(themes, r, p.theme.as_deref());
            let row_thm = themes.get(theme_name)?;
            let groups = parts::relation_groups(
                cfg,
                db,
                row_thm.schemas(),
                &resolve_asset,
                rel_groups.get(&p.url).cloned().unwrap_or_default(),
            )?;
            let doc = parts::document(p, whole, trail, groups);
            let dir = p.path.parent().unwrap_or(root);
            let (html_attrs, body_attrs) = (
                render::eval_attrs(&attrs.html, cfg, p, &site, &head.title, &p.url),
                render::eval_attrs(&attrs.body, cfg, p, &site, &head.title, &p.url),
            );
            let html = chain::document_page(
                chain::Page {
                    theme: row_thm,
                    head_html: render::head_html(
                        &head,
                        &css_urls.of(&cfg.site.baseurl, theme_name),
                        &cfg.html.head.include,
                    ),
                    site_title: site.title,
                    source_dir: dir,
                    lang: lang.as_str(),
                    html_attrs,
                    body_attrs,
                    resolve_link: &preview::fill_link_resolver(cfg, linkspace, lang.as_str()),
                    subtheme: subtheme.as_deref(),
                    profile,
                    axis: &r.axis,
                    axes: preview::axes_part(cfg, db, r),
                    chrome: preview::chrome_input(cfg, &chrome_facts, lang.as_str()),
                },
                cfg,
                Some(p),
                doc,
                whole,
                &resolve_asset,
                parts::FillOpts {
                    blocks: Some(bodies[&p.key].blocks.as_slice()),
                    thumbs: Some(thumbs),
                    ..Default::default()
                },
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
            metas,
            attrs,
            cfg,
            db,
            site,
            themes,
            thumbs,
            bodies,
            page_bodies,
            linkspace,
            backlinks,
            css_urls,
            root: root.to_path_buf(),
            profile,
            objects: db.object_ix.iter().collect(),
            chrome: &chrome_facts,
        };
        crate::passes::run(&ctx, &crate::passes::all(), out_map, stats)?;
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
        let loc_owned = cfg.pairing_member(r);
        let loc = loc_owned.as_str();

        // The claimed row, in the route's locale — else the default's
        // prose (the same fallback slot fills use).
        let sibs = db.by_logical.get(content).cloned().unwrap_or_default();
        let row = sibs
            .iter()
            .filter_map(|k| db.rows.get(k))
            .find(|p| cfg.pairing_member(*p) == loc)
            .or_else(|| {
                let canon = cfg.pairing_canonical().unwrap_or("");
                sibs.iter()
                    .filter_map(|k| db.rows.get(k))
                    .find(|p| cfg.pairing_member(*p) == canon)
            });
        let Some(row) = row else { continue }; // existence-checked at load
        let src = &row.path;

        if preview::view_base_collection(cfg, view).is_none() {
            continue;
        }
        let (theme_name, subtheme) =
            preview::resolve_view_theme(themes, r, v.theme.as_deref(), || {
                themes.resolve(row.theme.as_deref())
            });
        let row_thm = themes.get(theme_name)?;
        let items = preview::member_rows(
            cfg,
            row_thm.schemas(),
            &resolve_asset,
            db,
            view,
            &r.members,
            thumbs,
            bodies,
            page_bodies,
            |k| db.object_ix.iter().any(|o| o == k),
        )?;
        let layout = v
            .layout
            .as_deref()
            .with_context(|| format!("view {view}: landing embed needs a layout (member face)"))?;
        let mut embed_html = chain::member_faces(row_thm, layout, v.variant.as_deref(), &items)
            .with_context(|| format!("view {view}"))?;
        if let Some(p) = preview::pagination_parts(cfg, db, view, v, r)? {
            embed_html.push_str(&row_thm.fragments.render(&p));
        }

        // Must-place (q45): the claimed row owns the arrangement — a body
        // that never places the owner's embed strands the view's rows.
        // (A `default_content` claim only exists if the row already placed the
        // embed — see `resolve_default_content` — so this stays exact.)
        let text =
            std::fs::read_to_string(src).with_context(|| format!("reading {}", src.display()))?;
        let (_, _, body) = split_front_matter(&text);
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
            site: Some(site),
            thumbs: Some(thumbs),
            theme: Some(row_thm),
            widgets: Some(&cfg.widgets),
            cfg: Some(cfg),
            links: Some(linkspace),
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
            crate::links::resolve(cfg, linkspace, &dir, &r.url, loc, &rel, form, href)
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
        let pairing_keep = cfg
            .pairing_axis()
            .map(|(_, a)| (a.field.as_str(), a.canonical().unwrap_or("")));
        let section = section_parts(db, &mut section_trees, &row.rel, &r.url, pairing_keep);
        let groups = parts::relation_groups(
            cfg,
            db,
            row_thm.schemas(),
            &resolve_asset,
            rel_groups.get(&r.url).cloned().unwrap_or_default(),
        )?;
        let doc = parts::document_tree(
            cfg,
            loc,
            &crate::trails::home_url(cfg, db, loc),
            &title,
            &r.url,
            &crate::trails::ancestors(cfg, db, &r.url),
            section,
            groups,
            &frag,
        );
        let site = site.with_title(cfg.site_title(loc));
        let head = render::head_for(&title, &r.url, &site, metas, r);
        let dir = src.parent().unwrap_or(root);
        let (html_attrs, body_attrs) = (
            render::eval_attrs(&attrs.html, cfg, row, &site, &title, &r.url),
            render::eval_attrs(&attrs.body, cfg, row, &site, &title, &r.url),
        );
        let html = chain::document_page(
            chain::Page {
                theme: row_thm,
                head_html: render::head_html(
                    &head,
                    &css_urls.of(&cfg.site.baseurl, theme_name),
                    &cfg.html.head.include,
                ),
                site_title: site.title,
                source_dir: dir,
                lang: loc,
                html_attrs,
                body_attrs,
                resolve_link: &preview::fill_link_resolver(cfg, linkspace, loc),
                subtheme: subtheme.as_deref(),
                profile,
                axis: &r.axis,
                axes: preview::axes_part(cfg, db, r),
                chrome: preview::chrome_input(cfg, &chrome_facts, loc),
            },
            cfg,
            Some(row),
            doc,
            &frag,
            &resolve_asset,
            parts::FillOpts {
                thumbs: Some(thumbs),
                ..Default::default()
            },
        )?;
        out_map.insert(r.url.clone(), html.into_bytes());
        stats.listings += 1;
    }

    // ---- fold shells: atom, sitemap, registered scripts
    crate::shells::atom::emit(cfg, db, site, bodies, page_bodies, out_map, stats);
    crate::shells::sitemap::emit(cfg, db, site, out_map, stats);
    crate::shells::script::emit(cfg, db, site, bodies, page_bodies, root, out_map, stats)?;

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
                let pairing_keep = cfg
                    .pairing_axis()
                    .map(|(_, a)| (a.field.as_str(), a.canonical().unwrap_or("")));
                let section = row
                    .map(|p| section_parts(db, &mut section_trees, &p.rel, &r.url, pairing_keep))
                    .unwrap_or_default();

                // Theme per row (§5a); axis theme beats the row's (q53).
                let (theme_name, subtheme) =
                    preview::resolve_theme(themes, r, row.and_then(|p| p.theme.as_deref()));
                let row_thm = themes.get(theme_name)?;
                let row_css = css_urls.of(&cfg.site.baseurl, theme_name);
                let lang = row.map(|p| cfg.pairing_member(p)).unwrap_or_default();
                let lang = lang.as_str();
                let site = site.with_title(cfg.site_title(lang));
                // Metas read the ROW when present; sourceless routes use the route.
                let mut head = match row {
                    Some(p) => render::head_for(&title, &p.url, &site, metas, p),
                    None => render::head_for(&title, &r.url, &site, metas, r),
                };
                if let Some(doc) = &pb.doc {
                    head.injected = doc.heads.clone();
                }
                // IO.md §4: the output picks its map shell. `raw` is the
                // transparent one — the body IS the output, so an imported
                // document can carry front matter (title, tags, hidden)
                // without being nested inside a second `<html>`.
                // q53: an axis member over `shell` is the md twin's shape — the
                // same row serialized two ways, at two URLs. The member's value
                // beats the row's own for the same reason a member's theme
                // does: the member IS the alternative form.
                //
                let shell =
                    preview::axis_field(r, "shell").or(row.and_then(|p| p.shell.as_deref()));
                if shell == Some("raw") {
                    out_map.insert(r.url.clone(), frag.clone().into_bytes());
                    stats.pages += 1;
                    continue;
                }
                let tier = match shell {
                    Some("light_html") => Theme::Light,
                    _ => Theme::Default,
                };
                let attr_row: &dyn crate::filter::Row = match row {
                    Some(p) => p,
                    None => r,
                };
                let (html_attrs, body_attrs) = (
                    render::eval_attrs(&attrs.html, cfg, attr_row, &site, &title, &r.url),
                    render::eval_attrs(&attrs.body, cfg, attr_row, &site, &title, &r.url),
                );
                let html = match tier {
                    Theme::Light => {
                        chain::light_page(&head, &html_attrs, &body_attrs, profile, &r.axis, frag)
                    }
                    Theme::Default => {
                        let groups = parts::relation_groups(
                            cfg,
                            db,
                            row_thm.schemas(),
                            &resolve_asset,
                            rel_groups.get(&r.url).cloned().unwrap_or_default(),
                        )?;
                        let mut head = head;
                        head.alternates = prepass::head_alternates(
                            metas,
                            &site,
                            &title,
                            cfg,
                            db,
                            r,
                            &cfg.media_types,
                        );
                        let doc = parts::document_tree(
                            cfg,
                            lang,
                            &crate::trails::home_url(cfg, db, lang),
                            &title,
                            &r.url,
                            &crate::trails::ancestors(cfg, db, &r.url),
                            section,
                            groups,
                            frag,
                        );
                        let dir = src.parent().unwrap_or(root);
                        chain::document_page(
                            chain::Page {
                                theme: row_thm,
                                head_html: render::head_html(
                                    &head,
                                    &row_css,
                                    &cfg.html.head.include,
                                ),
                                site_title: site.title,
                                source_dir: dir,
                                lang,
                                html_attrs,
                                body_attrs,
                                resolve_link: &preview::fill_link_resolver(cfg, linkspace, lang),
                                subtheme: subtheme.as_deref(),
                                profile,
                                axis: &r.axis,
                                axes: preview::axes_part(cfg, db, r),
                                chrome: preview::chrome_input(cfg, &chrome_facts, lang),
                            },
                            cfg,
                            row,
                            doc,
                            frag,
                            &resolve_asset,
                            parts::FillOpts {
                                blocks: pb.doc.as_ref().map(|d| d.blocks.as_slice()),
                                thumbs: Some(thumbs),
                                ..Default::default()
                            },
                        )?
                    }
                };
                out_map.insert(r.url.clone(), html.into_bytes());
                stats.pages += 1;
            }
            _ => {}
        }
    }

    Ok(())
}

/// Section outline for a row inside a `.section` unit (§6e), cached per unit.
pub(crate) fn section_parts(
    db: &SiteDb,
    section_trees: &mut HashMap<PathBuf, Vec<crate::outline::Node>>,
    rel: &Path,
    page_url: &str,
    pairing_keep: Option<(&str, &str)>,
) -> Vec<parts::PartMap> {
    crate::outline::nearest(&db.sections, rel)
        .map(|sec| {
            let tree = section_trees
                .entry(sec.to_path_buf())
                .or_insert_with(|| crate::outline::section_tree(db, sec, pairing_keep));
            crate::outline::to_parts(tree, page_url)
        })
        .unwrap_or_default()
}

//! Aggregate pages: N rows, previewed (THEME.md §3).
//!
//! `layout` / `variant` only pick the member face; any layout the theme
//! ships a `row--*` face for is valid.

use anyhow::{Context, Result};

use super::{Ctx, Pass};
use crate::assemble::chain;
use crate::config::View;
use crate::model::Route;
use crate::parts;
use crate::passes::preview::{
    axes_part, chrome_input, fill_link_resolver, member_rows, pagination_parts, resolve_view_theme,
    route_intro,
};
use crate::pipeline::{SiteOutput, Stats};
use crate::render;

pub struct Listing;

impl Pass for Listing {
    fn render(
        &self,
        ctx: &Ctx,
        r: &Route,
        view: &str,
        v: &View,
        out: &mut SiteOutput,
        stats: &mut Stats,
    ) -> Result<()> {
        let cfg = ctx.cfg;
        let (theme_name, subtheme) = resolve_view_theme(ctx.themes, r, v.theme.as_deref(), || {
            match ctx.unanimous_theme(r) {
                Some(n) => (Some(n), None),
                None => ctx.themes.site_default(),
            }
        });
        let row_thm = ctx.themes.get(theme_name)?;
        let resolve_asset = |src: &str| {
            crate::thumbs::default_of(ctx.thumbs, src)
                .map(|t| t.url.clone())
                .unwrap_or_else(|| crate::passes::preview::asset_url(&cfg.site.baseurl, src))
        };
        let items = member_rows(
            cfg,
            row_thm.schemas(),
            &resolve_asset,
            ctx.db,
            view,
            &r.members,
            ctx.thumbs,
            ctx.bodies,
            ctx.page_bodies,
            |k| ctx.objects.contains(k),
        )?;
        let (title, trail) = crate::trails::listing_title_and_trail(cfg, ctx.db, view, v, r)?;
        let pagination = pagination_parts(cfg, ctx.db, view, v, r)?;
        let loc = ctx.pairing_member(r);
        let intro = route_intro(cfg, v, view, r, ctx.linkspace, loc.as_str())?;
        let layout = v
            .layout
            .as_deref()
            .context("listing pass only sees layouted views")?;
        let content = chain::member_faces(row_thm, layout, v.variant.as_deref(), &items)
            .with_context(|| format!("view {view}"))?;
        let main = row_thm.fragments.render(&parts::page_row(
            &title, &r.url, trail, intro, content, pagination,
        ));
        let site = ctx.site.with_title(cfg.site_title(loc.as_str()));
        let head = render::head_for(&title, &r.url, &site, ctx.metas, r);
        let resolve = fill_link_resolver(cfg, ctx.linkspace, loc.as_str());
        let html_attrs = render::eval_attrs(&ctx.attrs.html, cfg, r, &site, &title, &r.url);
        let body_attrs = render::eval_attrs(&ctx.attrs.body, cfg, r, &site, &title, &r.url);
        let html = chain::wrap(
            chain::Page {
                theme: row_thm,
                head_html: render::head_html(
                    &head,
                    &ctx.css_of(theme_name),
                    &cfg.html.head.include,
                ),
                site_title: site.title,
                source_dir: ctx.root_path(),
                lang: loc.as_str(),
                html_attrs,
                body_attrs,
                resolve_link: &resolve,
                subtheme: subtheme.as_deref(),
                profile: ctx.profile,
                axis: &r.axis,
                axes: axes_part(cfg, ctx.db, r),
                chrome: chrome_input(cfg, ctx.chrome, loc.as_str()),
            },
            main,
        )?;
        out.insert(r.url.clone(), html.into_bytes());
        stats.listings += 1;
        Ok(())
    }
}

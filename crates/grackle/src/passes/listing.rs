//! Aggregate pages: N rows, previewed (THEME.md §3).
//!
//! `layout` / `variant` only pick the member face; any layout the theme
//! ships a `row--*` face for is valid.

use anyhow::{Context, Result};

use super::{Ctx, Pass};
use crate::assemble::chain;
use crate::build::{
    axes_part, fill_link_resolver, member_previews, pagination_parts, resolve_view_theme,
    route_intro, SiteOutput, Stats,
};
use crate::config::View;
use crate::db::Route;
use crate::parts;
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
        let items = member_previews(
            cfg,
            ctx.db,
            view,
            &r.members,
            ctx.thumbs,
            ctx.bodies,
            ctx.page_bodies,
            |k| ctx.objects.contains(k),
        );
        let (title, trail) = crate::trails::listing_title_and_trail(cfg, ctx.db, view, v, r)?;
        let pagination = pagination_parts(ctx.db, view, v, r)?;
        let loc = ctx.locale_of(r);
        let intro = route_intro(cfg, v, view, r, ctx.linkspace, loc)?;
        let (theme_name, subtheme) = resolve_view_theme(ctx.themes, r, v.theme.as_deref(), || {
            match ctx.unanimous_theme(r) {
                Some(n) => (Some(n), None),
                None => ctx.themes.site_default(),
            }
        });
        let row_thm = ctx.themes.get(theme_name)?;
        let layout = v
            .layout
            .as_deref()
            .context("listing pass only sees layouted views")?;
        let content = chain::member_faces(
            &row_thm.fragments,
            layout,
            v.variant.as_deref(),
            items,
        )
        .with_context(|| format!("view {view}"))?;
        let main = row_thm.fragments.render(&parts::page_row(
            &title,
            &r.url,
            trail,
            intro,
            content,
            pagination,
        ));
        let head = render::head_for(&title, &r.url, ctx.site, ctx.metas, r);
        let resolve = fill_link_resolver(cfg, ctx.linkspace, loc);
        let html = chain::wrap(
            chain::Page {
                theme: row_thm,
                head_html: render::head_html(&head, &ctx.css_of(theme_name)),
                site_title: &cfg.site.title,
                source_dir: ctx.root_path(),
                locale: loc,
                resolve_link: &resolve,
                subtheme: subtheme.as_deref(),
                profile: ctx.profile,
                axis: &r.axis,
                axes: axes_part(cfg, ctx.db, r),
            },
            main,
        )?;
        out.insert(r.url.clone(), html.into_bytes());
        stats.listings += 1;
        Ok(())
    }
}

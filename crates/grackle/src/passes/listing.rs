//! `layout = "listing"`: N rows, previewed.

use anyhow::Result;

use super::{Ctx, Pass};
use crate::build::{fill_link_resolver, member_previews, pagination_parts, route_intro, SiteOutput, Stats};
use crate::config::View;
use crate::db::Route;
use crate::parts;
use crate::render;

pub struct Listing;

impl Pass for Listing {
    fn layout(&self) -> &'static str {
        "listing"
    }

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

        // Nearest wins (§5e): view theme, then unanimous members (§5h), then site.
        // Axis theme (§53) beats view/unanimity — the member IS this route.
        let (theme_name, subtheme) =
            match crate::build::axis_field(r, "theme").or(v.theme.as_deref()) {
                Some(spec) => ctx.themes.resolve(Some(spec)),
                None => match ctx.unanimous_theme(r) {
                    Some(n) => (Some(n), None),
                    None => ctx.themes.site_default(),
                },
            };
        let row_thm = ctx.themes.get(theme_name)?;
        let face = parts::member_face("listing", v.variant.as_deref());
        let content =
            crate::assemble::chain::concat_rows(&row_thm.fragments, face, items, v.featured);
        let main = row_thm
            .fragments
            .render(&parts::page_row(
                &title,
                &r.url,
                trail,
                intro,
                content,
                pagination,
            ));
        let mut head = render::head_simple(&title, &r.url, ctx.site);
        head.meta = render::eval_metas(ctx.metas, r, ctx.site, &title, &r.url);
        let html = row_thm.page(
            render::head_html(&head, &ctx.css_of(theme_name)),
            &cfg.site.title,
            main,
            ctx.root_path(),
            loc,
            &fill_link_resolver(cfg, ctx.linkspace, loc),
            subtheme.as_deref(),
            ctx.profile,
            &r.axis,
            crate::build::axes_part(cfg, ctx.db, r),
        )?;
        out.insert(r.url.clone(), html.into_bytes());
        stats.listings += 1;
        Ok(())
    }
}

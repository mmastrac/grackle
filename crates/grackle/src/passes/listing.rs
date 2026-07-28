//! `layout = "listing"`: N rows, previewed.

use anyhow::Result;

use super::{Ctx, Pass};
use crate::assemble::chain;
use crate::build::{
    axes_part, fill_link_resolver, member_previews, pagination_parts, route_intro, SiteOutput,
    Stats,
};
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
        let content = chain::concat_rows(&row_thm.fragments, face, items, v.featured);
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

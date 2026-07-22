//! `layout = "gallery"`: object-backed views.
//!
//! The view supplied the query (`match` glob, filter, `order_by`); this only
//! shapes rows into `figure` parts. Dimensions come from the thumb pass so the
//! browser reserves space and masonry never shifts.

use anyhow::Result;

use super::{Ctx, Pass};
use crate::build::{route_intro, fill_link_resolver, SiteOutput, Stats};
use crate::config::View;
use crate::db::Route;
use crate::parts;
use crate::render;

pub struct Gallery;

impl Pass for Gallery {
    fn layout(&self) -> &'static str {
        "gallery"
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
        let (title, trail) = crate::trails::listing_title_and_trail(ctx.cfg, ctx.db, view, v, r)?;
        let items: Vec<parts::Figure> = r
            .members
            .iter()
            .filter_map(|k| ctx.db.rows.get(k))
            .map(|o| {
                let t = ctx.thumbs.get(&o.rel.to_string_lossy().to_string());
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
        let loc = ctx.locale_of(r);
        let intro = route_intro(ctx.cfg, v, view, r, ctx.linkspace, loc)?;
        let main = ctx.thm.fragments.render_with(
            &parts::gallery(&items, &title, trail, intro),
            v.variant.as_deref(),
        );
        let head = render::head_simple(&title, &r.url, ctx.site, false);
        let html = ctx.thm.page(
            render::head_html(&head, &ctx.css_of(None)),
            &ctx.cfg.site.title,
            main,
            ctx.root_path(),
            loc,
            &fill_link_resolver(ctx.cfg, ctx.linkspace, loc),
            None,
            ctx.profile,
        )?;
        out.insert(r.url.clone(), html.into_bytes());
        stats.listings += 1;
        Ok(())
    }
}

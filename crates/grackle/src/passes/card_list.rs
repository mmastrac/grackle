//! `layout = "card_list"`: tree-backed views where the first member leads.
//!
//! The book of the month renders large, the back catalogue follows as cards.

use anyhow::Result;

use super::{Ctx, Pass};
use crate::build::{route_intro, fill_link_resolver, SiteOutput, Stats};
use crate::config::View;
use crate::db::Route;
use crate::parts;
use crate::render;

pub struct CardList;

impl Pass for CardList {
    fn layout(&self) -> &'static str {
        "card_list"
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
        let rows: Vec<parts::CardRow> = r
            .members
            .iter()
            .filter_map(|k| ctx.db.rows.get(k))
            .map(|p| {
                let t = p.hero_source().and_then(|s| ctx.thumbs.get(s));
                parts::CardRow {
                    title: p.title.clone().unwrap_or_default(),
                    url: p.url.clone(),
                    src: t.map(|t| t.url.clone()),
                    dims: t.and_then(|t| t.dims),
                    note: p.description.clone(),
                }
            })
            .collect();
        let loc = ctx.locale_of(r);
        let intro = route_intro(ctx.cfg, v, view, r, ctx.linkspace, loc)?;
        let theme_name = ctx.unanimous_theme(r);
        let row_thm = ctx.themes.get(theme_name)?;
        let main = row_thm.fragments.render_with(
            &parts::featured_listing(&rows, v.featured, &title, trail, intro),
            v.variant.as_deref(),
        );
        let head = render::head_simple(&title, &r.url, ctx.site, false);
        let html = row_thm.page(
            render::head_html(&head, &ctx.css_of(theme_name)),
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

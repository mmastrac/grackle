//! `layout = "listing"`: N rows, summarised.

use anyhow::Result;

use super::{Ctx, Pass};
use crate::build::{route_intro, fill_link_resolver, pagination_parts, SiteOutput, Stats};
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
        let db = ctx.db;

        // The preview is the row's computed `summary` field (§6d): a derived
        // column the view declares, or inherits along `over`. `truncated`
        // rides along as the deriver's fact, gating the theme's ★. No summary
        // field in the chain = rows ship whole.
        let summary_field = cfg.fields_for(view).get("summary").and_then(|f| f.truncate);
        let rows: Vec<(&crate::db::Row, String, bool)> = r
            .members
            .iter()
            .filter_map(|k| db.rows.get(k))
            .map(|p| match ctx.bodies.get(p.url.as_str()) {
                Some(d) => match summary_field {
                    Some(t) => {
                        let (html, truncated) = d.truncate(t.max_blocks, t.max_chars);
                        (p, html, truncated)
                    }
                    None => (p, d.whole.clone(), false),
                },
                // Tree row bodies are re-read rather than held (§2), so a
                // listing over tree rows finds them in the other map.
                None => (
                    p,
                    ctx.page_bodies
                        .get(&p.url)
                        .map(|pb| pb.frag.clone())
                        .unwrap_or_default(),
                    false,
                ),
            })
            .collect();

        let (title, trail) = crate::trails::listing_title_and_trail(cfg, db, view, v, r)?;
        let pagination = pagination_parts(db, view, v, r)?;
        let loc = ctx.locale_of(r);
        let intro = route_intro(cfg, v, view, r, ctx.linkspace, loc)?;

        let main = ctx.thm.fragments.render_with(
            &parts::listing(cfg, &rows, &title, trail, intro, pagination),
            v.variant.as_deref(),
        );
        let head = render::head_simple(&title, &r.url, ctx.site, view != "blog_index");
        let html = ctx.thm.page(
            render::head_html(&head, &ctx.css_of(None)),
            &cfg.site.title,
            main,
            ctx.root_path(),
            loc,
            &fill_link_resolver(cfg, ctx.linkspace, loc),
            None,
            ctx.profile,
        )?;
        out.insert(r.url.clone(), html.into_bytes());
        stats.listings += 1;
        Ok(())
    }
}

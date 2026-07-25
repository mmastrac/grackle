//! `layout = "listing"`: N rows, previewed.
//!
//! One pass for every arrangement of previews — the blog index, the photo
//! gallery and the book club. What differs is what each member can answer,
//! which is q36's settlement: a row with prose previews as prose, a row that
//! IS a picture previews as one.

use anyhow::Result;

use super::{Ctx, Pass};
use crate::build::{
    fill_link_resolver, object_preview, pagination_parts, route_intro, row_preview, SiteOutput,
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
        let db = ctx.db;

        // The prose preview is the row's computed `summary` field (§6d): a
        // derived column the view declares, or inherits along `over`. No
        // summary field in the chain = rows ship whole.
        let summary_field = cfg.fields_for(view).get("summary").and_then(|f| f.truncate);
        let items: Vec<parts::Preview> = r
            .members
            .iter()
            .filter_map(|k| db.rows.get(k))
            .map(|p| {
                if ctx.objects.contains(&p.key) {
                    return object_preview(p, ctx.thumbs);
                }
                let (html, truncated) = match ctx.bodies.get(p.url.as_str()) {
                    Some(d) => match summary_field {
                        Some(t) => d.truncate(t.max_blocks, t.max_chars),
                        None => (d.whole.clone(), false),
                    },
                    // Tree row bodies are re-read rather than held (§2), so a
                    // listing over tree rows finds them in the other map.
                    None => (
                        ctx.page_bodies
                            .get(&p.url)
                            .map(|pb| pb.frag.clone())
                            .unwrap_or_default(),
                        false,
                    ),
                };
                row_preview(cfg, p, ctx.thumbs, Some(html), truncated)
            })
            .collect();

        let (title, trail) = crate::trails::listing_title_and_trail(cfg, db, view, v, r)?;
        let pagination = pagination_parts(db, view, v, r)?;
        let loc = ctx.locale_of(r);
        let intro = route_intro(cfg, v, view, r, ctx.linkspace, loc)?;

        let theme_name = ctx.unanimous_theme(r);
        let row_thm = ctx.themes.get(theme_name)?;
        let main = row_thm.fragments.render_with(
            &parts::listing(items, v.featured, &title, trail, intro, pagination),
            v.variant.as_deref(),
        );
        let mut head = render::head_simple(&title, &r.url, ctx.site);
        head.meta = render::eval_metas(ctx.metas, r, ctx.site, &title, &r.url);
        let html = row_thm.page(
            render::head_html(&head, &ctx.css_of(theme_name)),
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

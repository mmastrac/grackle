//! The `sitemap` fold shell: XML serialization of the finished route set.

use std::fmt::Write as _;

use crate::config::Config;
use crate::model::SiteDb;
use crate::pipeline::types::{SiteOutput, Stats};
use crate::render::{self, Site};

/// The XML sitemap (sitemap.xml). Entries are `(absolute loc, optional
/// lastmod)`, already in the order they should appear.
pub fn xml(entries: &[(String, Option<String>)]) -> String {
    let mut s = String::with_capacity(64 * 1024);
    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str("<urlset xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:schemaLocation=\"http://www.sitemaps.org/schemas/sitemap/0.9 http://www.sitemaps.org/schemas/sitemap/0.9/sitemap.xsd\" xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n");
    for (loc, lastmod) in entries {
        s.push_str("<url>\n");
        let _ = writeln!(s, "<loc>{loc}</loc>");
        if let Some(lm) = lastmod {
            let _ = writeln!(s, "<lastmod>{lm}</lastmod>");
        }
        s.push_str("</url>\n");
    }
    s.push_str("</urlset>\n");
    s
}

/// Emit every `shell = "sitemap"` fold into `out_map`.
///
/// The fold (§5) counted its matches at load; here we read them back. `lastmod`
/// is emitted only for dated rows, from the content date. jekyll-sitemap also
/// stamps static files with their file *mtime* — but that is checkout-time
/// noise (every clone differs) and works against the indexing goal this whole
/// project exists for, so it is deliberately dropped — the URL *set* is
/// unaffected. (DESIGN §4a is the related draft/hidden concern.)
pub(crate) fn emit(
    cfg: &Config,
    db: &SiteDb,
    site: &Site<'_>,
    out_map: &mut SiteOutput,
    stats: &mut Stats,
) {
    for fold in &db.routes {
        let Some(view) = &fold.view else { continue };
        let Some(v) = cfg.views.get(view) else {
            continue;
        };
        if v.shell.as_deref() != Some("sitemap") {
            continue;
        }
        // Resolved at load like every other view's, rather than re-derived
        // from the filter's source text here.
        let entries: Vec<(String, Option<String>)> = fold
            .route_members
            .iter()
            .filter_map(|k| db.routes.get(k))
            .map(|r| {
                let loc = format!("{}{}", site.url, r.url);
                // `lastmod` follows the date, not the table: a dated tree row
                // gets one too. Same `modified` source the feed's `<updated>`
                // reads, so the two agree by construction.
                let lastmod = db
                    .row_by_url(&r.url)
                    .and_then(super::modified::of)
                    .map(render::xmlschema);
                (loc, lastmod)
            })
            .collect();
        let body = xml(&entries);
        out_map.insert(fold.url.clone(), body.into_bytes());
        stats.serialized += 1;
    }
}

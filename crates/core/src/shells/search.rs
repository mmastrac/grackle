//! The `search` fold shell: postcard index + wasm/js assets.

use anyhow::Result;
use std::collections::HashMap;

use crate::config::Config;
use crate::markdown::Doc;
use crate::model::{Row, SiteDb};
use crate::pipeline::types::{PageBody, SiteOutput, Stats};

fn search_doc(cfg: &Config, p: &Row, body: &str) -> grackle_search_core::SearchDoc {
    grackle_search_core::SearchDoc {
        url: p.url.clone(),
        store: cfg.search_store(p, body),
        streams: cfg.search_streams(p, body),
    }
}

/// The searchable projection of the posts table — the CLI smoke query
/// (`grackle query search`), which runs no render pass and feeds raw
/// markdown. The SHIPPED index is not this: it is the `shell = "search"`
/// view's serialization (see [`search_pass`]), which may span tables.
pub fn search_docs(
    cfg: &Config,
    db: &SiteDb,
    html_of: impl Fn(&Row) -> String,
) -> Vec<grackle_search_core::SearchDoc> {
    db.posts()
        .map(|p| {
            let html = html_of(p);
            let body = grackle_search_core::strip_tags(&html);
            search_doc(cfg, p, &body)
        })
        .collect()
}

/// Search (§6b, §5g): the index is a SHELL — a view declares
/// `shell = "search"` with a filter over the route schema (the sitemap's
/// shape), and the rows that pass are the searchable set, serialized as
/// postcard at the view's route. Posts and pages carry bodies; other route
/// kinds are silently unsearchable even if the filter admits them. The
/// wasm consumer + /search.js loader are engine assets embedded in the
/// binary (they must version with the index format), emitted only when a
/// search view exists, fetched only when a theme's trigger is clicked.
/// Search-asset version, carried in the wasm and bin URLs so they bust their
/// caches TOGETHER. The wasm reads the bin, and a fresh wasm against a
/// cache-stale bin is a hard "bad index" — which is what happened when the
/// index format changed and Cloudflare served a day-old bin beside a fresh
/// wasm. A version in both filenames makes a format change a URL change, so no
/// cache can hand back a mismatched pair. **Bump on any change to the
/// `search-core` on-disk format**; the site's `[routes.search] path` must be
/// `/search.{SEARCH_VER}.bin` to match what `search.js` fetches. (A stopgap
/// until q54 makes derived-asset URLs content-addressed and retires it.)
pub(crate) const SEARCH_VER: &str = "v1";

pub(crate) fn search_pass(
    cfg: &Config,
    db: &SiteDb,
    bodies: &HashMap<&grackle_db::Key, Doc>,
    page_bodies: &HashMap<String, PageBody>,
    out_map: &mut SiteOutput,
    stats: &mut Stats,
) -> Result<()> {
    let mut any = false;
    for fold in &db.routes {
        let Some(view) = &fold.view else { continue };
        let Some(v) = cfg.views.get(view) else {
            continue;
        };
        if v.shell.as_deref() != Some("search") {
            continue;
        }
        let route = &fold.url;
        // Resolved at load, like the sitemap's.
        let docs: Vec<grackle_search_core::SearchDoc> = fold
            .route_members
            .iter()
            .filter_map(|k| db.routes.get(k))
            // The two arms are two BODY STORES, not two kinds of thing to say
            // about an output — which is why `kind` survives I13 here: a post's
            // html is in `bodies` (keyed by row) and a page's in `page_bodies`
            // (keyed by URL), and no fact on the route says which pass filled
            // which. `_ => None` is the rest: a byte copy has no body to
            // search, and a fold's output is not a document.
            .filter_map(|r| match r.kind {
                crate::model::RouteKind::Post => {
                    r.row.as_ref().and_then(|k| db.rows.get(k)).map(|p| {
                        let html = bodies.get(&p.key).map(|d| d.whole.as_str()).unwrap_or("");
                        let body = grackle_search_core::strip_tags(html);
                        search_doc(cfg, p, &body)
                    })
                }
                crate::model::RouteKind::Page => {
                    let pb = page_bodies.get(&r.url).filter(|pb| !pb.skipped)?;
                    let p = r.row.as_ref().and_then(|k| db.rows.get(k))?;
                    let html = pb
                        .doc
                        .as_ref()
                        .map(|d| d.whole.as_str())
                        .unwrap_or(pb.frag.as_str());
                    let body = grackle_search_core::strip_tags(html);
                    Some(search_doc(cfg, p, &body))
                }
                _ => None,
            })
            .collect();
        let t = std::time::Instant::now();
        let (index, st) = grackle_search_core::build_index(&docs);
        let bin = index.to_bytes();
        stats.search_bytes = bin.len();
        println!(
            "  search    {} docs, {} terms, {} postings -> {} KB in {:.0}ms",
            st.docs,
            st.terms,
            st.postings,
            bin.len() / 1024,
            t.elapsed().as_secs_f64() * 1000.0
        );
        out_map.insert(route.clone(), bin);
        any = true;
    }
    if any {
        out_map.insert("/search.js".to_string(), search_js(cfg));
        out_map.insert(
            format!("/search.{SEARCH_VER}.wasm"),
            include_bytes!("../../assets/search.wasm").to_vec(),
        );
    }
    Ok(())
}

/// `/search.js` with `[i18n.strings]` search vocabulary baked in per locale.
fn search_js(cfg: &Config) -> Vec<u8> {
    let members: Vec<&str> = match cfg.pairing_axis() {
        Some((_, a)) => a.values.iter().map(String::as_str).collect(),
        None => vec![""],
    };
    let mut map = serde_json::Map::new();
    for m in &members {
        let entry = serde_json::json!({
            "label": cfg.i18n_string("search", m),
            "placeholder": cfg.i18n_string("search_placeholder", m),
            "empty": cfg.i18n_string("search_empty", m),
        });
        map.insert((*m).to_string(), entry);
    }
    // Empty key: fallback when `<html lang>` is unset or unknown.
    if !map.contains_key("") {
        let canon = cfg.pairing_canonical().unwrap_or("");
        if let Some(v) = map.get(canon).cloned() {
            map.insert("".into(), v);
        } else if let Some((_, v)) = map.iter().next() {
            map.insert("".into(), v.clone());
        }
    }
    let json = serde_json::Value::Object(map).to_string();
    // Every occurrence, not just the first: a stray mention of the sentinel in
    // a comment used to shadow the real assignment, leaving `var I18N =
    // __SEARCH_I18N__;` as a load-time ReferenceError that killed search.
    let filled = include_str!("../../assets/search.js")
        .replace("__SEARCH_I18N__", &json)
        .replace("__SEARCH_VER__", SEARCH_VER);
    debug_assert!(
        !filled.contains("__SEARCH_I18N__") && !filled.contains("__SEARCH_VER__"),
        "search.js sentinel not substituted"
    );
    filled.into_bytes()
}

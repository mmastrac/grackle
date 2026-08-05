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
    db.rows
        .iter()
        .filter(|p| cfg.body_held(p))
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
///
/// **The bin URL is the route's own, derived at emission** — the author
/// declares `[routes.search] path` and `search.js` fetches exactly that,
/// so the two cannot disagree. (They used to: the loader hardcoded
/// `/search.{SEARCH_VER}.bin` and any route declaring another path shipped
/// a working index the loader 404'd on.) The format firebreak rides a
/// query instead of the path: the fetch URL carries `?{SEARCH_VER}`, so a
/// format bump busts every cache without dictating the site's URL. The wasm
/// is content-addressed and busts on its own bytes; a fresh wasm against a
/// cache-stale bin is a hard "bad index", which is what happened when the
/// index format changed and Cloudflare served a day-old bin beside a fresh
/// wasm. Both URLs live in `/search.js`, so a client holds an all-old or
/// all-new pair, never mixed. Bump on any change to the `search-core`
/// on-disk format.
pub(crate) const SEARCH_VER: &str = "v1";

pub(crate) fn search_pass(
    cfg: &Config,
    db: &SiteDb,
    bodies: &HashMap<&grackle_db::Key, Doc>,
    page_bodies: &HashMap<String, PageBody>,
    out_map: &mut SiteOutput,
    stats: &mut Stats,
) -> Result<()> {
    let mut bin_route: Option<String> = None;
    for fold in &db.routes {
        let Some(view) = &fold.view else { continue };
        let Some(v) = cfg.views.get(view) else {
            continue;
        };
        if v.shell.as_deref() != Some("search") {
            continue;
        }
        let route = &fold.url;
        match &bin_route {
            None => bin_route = Some(route.clone()),
            Some(first) => eprintln!(
                "grackle: two search routes ({first} and {route}) — /search.js \
                 fetches the first"
            ),
        }
        // Resolved at load, like the sitemap's.
        let docs: Vec<grackle_search_core::SearchDoc> = fold
            .route_members
            .iter()
            .filter_map(|k| db.routes.get(k))
            .filter_map(|r| {
                let p = r.row.as_ref().and_then(|k| db.rows.get(k))?;
                if let Some(doc) = bodies.get(&p.key) {
                    let body = grackle_search_core::strip_tags(&doc.whole);
                    return Some(search_doc(cfg, p, &body));
                }
                let pb = page_bodies.get(&r.url).filter(|pb| !pb.skipped)?;
                let html = pb
                    .doc
                    .as_ref()
                    .map(|d| d.whole.as_str())
                    .unwrap_or(pb.frag.as_str());
                let body = grackle_search_core::strip_tags(html);
                Some(search_doc(cfg, p, &body))
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
    }
    if let Some(bin_route) = bin_route {
        // The wasm never changes between content edits, so it is the ideal
        // immutable target: content-address it and let its bytes bust it. The
        // firebreak is `/search.js` itself, which keeps its stable URL, so this
        // hash lands in one file and no HTML page (DESIGN.md q54).
        let wasm = include_bytes!("../../assets/search.wasm");
        let wasm_url =
            grackle_source::strong::address(wasm, grackle_source::strong::IDENTITY, "wasm");
        out_map.insert(
            "/search.js".to_string(),
            search_js(cfg, &wasm_url, &bin_route),
        );
        out_map.insert(wasm_url, wasm.to_vec());
    }
    Ok(())
}

/// `/search.js` with `[i18n.strings]` vocabulary baked in per locale and both
/// fetch URLs filled in: the content-addressed wasm, and the search route's
/// own bin path with the format version as a cache-busting query. Both carry
/// `baseurl`, because the loader runs in the browser where root-relative
/// means the wrong site under a prefix.
fn search_js(cfg: &Config, wasm_url: &str, bin_route: &str) -> Vec<u8> {
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
        .replace(
            "__SEARCH_BIN_URL__",
            &format!("{}{bin_route}?{SEARCH_VER}", cfg.site.baseurl),
        )
        .replace(
            "__SEARCH_WASM_URL__",
            &format!("{}{wasm_url}", cfg.site.baseurl),
        );
    debug_assert!(
        !filled.contains("__SEARCH_I18N__")
            && !filled.contains("__SEARCH_BIN_URL__")
            && !filled.contains("__SEARCH_WASM_URL__"),
        "search.js sentinel not substituted"
    );
    filled.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The loader fetches the bin at the route's own path — with `baseurl`,
    /// the format version as a cache-busting query, and the wasm prefixed the
    /// same way. No sentinel survives emission.
    #[test]
    fn search_js_fetches_the_routes_own_bin_url() {
        let cfg = Config::from_toml(
            "extends = \"none\"\n[site]\nurl = \"https://example.com\"\n\
             baseurl = \"/sub\"\ntitle = \"t\"\nauthor = \"a\"",
        )
        .expect("config parses");
        let js = String::from_utf8(search_js(&cfg, "/static/abc.wasm", "/find.bin")).unwrap();
        assert!(
            js.contains(&format!("fetch(\"/sub/find.bin?{SEARCH_VER}\")")),
            "{js}"
        );
        assert!(js.contains("fetch(\"/sub/static/abc.wasm\")"), "{js}");
        assert!(!js.contains("__SEARCH_"), "unfilled sentinel:\n{js}");
    }
}

//! Post-emit: citations, on-demand publish; orchestrates search + CSS shells.

use anyhow::{Context, Result};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use crate::config::Config;
use crate::markdown::Doc;
use crate::model::{Route, RouteKind, SiteDb};
use crate::pipeline::types::{PageBody, SiteOutput, Stats};

/// The citation half of `Route.inputs` (IO.md §2), added once the bytes exist.
///
/// **Facts at planning; content at materialization** — and `inputs` is the one
/// join field that straddles the line. `load::join_arrangement` filled every
/// edge planning knows (the row a route renders, a landing's claimed body, a
/// view's members, the rows behind a fold's selected routes); the rest of the
/// row-level closure is *cited* rows, and a citation is a fact about finished
/// output. An `{% image %}` expands to markup no body contains, so there is no
/// earlier moment this could be honest.
///
/// The same scanner on-demand publishing uses, and deliberately the unfenced
/// one (`cited_urls`, not `cited_urls_cited`): a spliced arrangement's links
/// are not citations for the backlink graph's purpose, but an image a listing
/// arranged is still an input to the bytes. What §6g's fence keeps out of
/// "linked from" it must not keep out of "what would a rebuild need".
///
/// Cited URLs that name no row are skipped rather than recorded: an
/// output→output edge is `route_members`, and an external link is not an edge
/// at all.
pub(crate) fn join_citations(db: &mut SiteDb, cited: &[(String, Vec<String>)]) {
    let mut found: Vec<(grackle_db::Key, Vec<grackle_db::Key>)> = Vec::new();
    for (url, urls) in cited {
        let rows: Vec<grackle_db::Key> =
            urls.iter().flat_map(|u| resolve_citation(db, u)).collect();
        if rows.is_empty() {
            continue;
        }
        found.push((grackle_db::Key::new(url), rows));
    }
    for (route, rows) in found {
        let Some(r) = db.routes.get_mut(&route) else {
            continue;
        };
        r.inputs.extend(rows);
        r.inputs.sort();
        r.inputs.dedup();
    }
}

/// The inputs one cited URL names (IO.md §4a, I11) — the address resolution
/// both halves of the citation seam read, so the pull and the join cannot
/// disagree about what an address means.
///
/// Two slots, two indexes. `by_url` answers a CANONICAL address and answers it
/// uniquely. `by_strong` answers a hash address and answers it with a LIST,
/// because the address is a pure function of the bytes and inputs holding one
/// byte string share it. All of them are edges: over-approximating here costs
/// a rebuild an extra output to reconsider, while dropping one is the stale
/// page the graph exists to prevent.
///
/// A URL in neither is not an edge at all — an external link, or an output
/// (whose edge is `route_members`).
pub(crate) fn resolve_citation(db: &SiteDb, url: &str) -> Vec<grackle_db::Key> {
    if let Some(k) = db.by_url.get(url) {
        return vec![k.clone()];
    }
    db.by_strong.get(url).cloned().unwrap_or_default()
}

/// Every internal citation of every finished output, by the URL it was
/// published at.
///
/// Each document is scanned against its OWN url, because a citation is usually
/// relative: `code/legacy/romtool/index.html` says `<img src="screen1.png">`,
/// which §6a records as how that content has always been organised. Binary
/// entries fail the UTF-8 gate and cite nothing.
///
/// One scan, two consumers (on-demand publishing and IO.md §2's citation
/// edges) — the seam that keeps the join's closure from costing a second pass
/// over the whole site.
pub(crate) fn citation_map(out: &SiteOutput, site_url: &str) -> Vec<(String, Vec<String>)> {
    out.iter()
        .filter_map(|(u, b)| std::str::from_utf8(b).ok().map(|t| (u, t)))
        .filter_map(|(u, t)| {
            let c = cited_urls(t, u, site_url);
            (!c.is_empty()).then(|| (u.clone(), c))
        })
        .collect()
}

/// Remove the `{% view %}` fence comments from the finished output (§6g). Only
/// touches the few pages that carry them; binary entries fail the UTF-8 gate
/// and are skipped.
pub(crate) fn strip_view_markers(out: &mut SiteOutput) {
    for bytes in out.values_mut() {
        let Ok(s) = std::str::from_utf8(bytes) else {
            continue;
        };
        if !s.contains("<!--grackle:view-->") {
            continue;
        }
        *bytes = s
            .replace("<!--grackle:view-->", "")
            .replace("<!--/grackle:view-->", "")
            .into_bytes();
    }
}

/// **Renditions as outputs** (IO.md §4a, I12): the artifacts `thumbs_pass`
/// published, entered into the outputs database with the edges and the
/// parameters that say what they are.
///
/// Two halves, and the pair is the whole model:
///
/// 1. **A rendition is an output of its input.** One `Route` per distinct
///    address, carrying `inputs` — the rows whose bytes fed the transform —
///    and `rendition`, the parameters it was made with. That pair is the
///    reproduction recipe: read those bytes, run `thumbs::render` with those
///    parameters, get these bytes back. The edge runs **input → output**,
///    because the transform reads the INPUT's bytes and never the original
///    output's; see `graph.rs` for what that answers.
/// 2. **The citing edge names it.** An output whose finished bytes embed a
///    rendition address gains a FACTS edge to it (`route_members`) — it read
///    the rendition's *url*, which the hashing law makes knowable at planning,
///    and not its content — plus the CONTENT edges to the rows behind it,
///    because those bytes are what its address is a function of.
///
/// The second half's content edges are load-bearing where an affordance shows
/// a rendition and links nothing else: a LISTING with a hero picture cites only
/// `/static/{hash}`, so without this its `inputs` would lose the image and
/// editing that image would ship a stale listing. `{% image %}` happens to link
/// the original beside its thumbnail, which is why the same edge arrives twice
/// on a post page and exactly once here.
///
/// Runs at build, like `materialize_referenced` and for the same reason: a
/// rendition exists because a citation asked for it, and citations live in
/// finished bytes. So the CLI's load-only surfaces (`explain`, `query pull`)
/// do not see these outputs, which is what they already do for on-demand ones.
pub(crate) fn join_renditions(
    db: &mut SiteDb,
    thumbs: &crate::thumbs::Renditions,
    cited: &[(String, Vec<String>)],
) {
    if thumbs.is_empty() {
        return;
    }
    // Rung 0 reaches this seam too (IO.md I10's law, stated at load.rs's
    // `force_route_fields`): minting an output is the graph event, so a seam
    // that mints applies the profile's forced fields. I11 added a shape to an
    // existing seam; this is the third seam, and the law is why it is one line
    // rather than a rediscovery.
    let forced: Vec<(String, grackle_db::filter::Value)> = db
        .forced_fields
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    // One output per address; its inputs are every source whose bytes landed
    // there. The union is over ROWS, so a source no rule admitted contributes
    // an artifact with no edge rather than a dangling key.
    let mut by_address: BTreeMap<String, (grackle_model::Rendition, Vec<grackle_db::Key>)> =
        BTreeMap::new();
    for (ask, t) in thumbs {
        let e = by_address
            .entry(t.address.clone())
            .or_insert_with(|| (ask.rendition, Vec::new()));
        let key = grackle_db::Key::new(&ask.src);
        if db.rows.get(&key).is_some() {
            e.1.push(key);
        }
    }
    for (address, (rendition, inputs)) in &mut by_address {
        inputs.sort();
        inputs.dedup();
        if db.routes.get(&grackle_db::Key::new(address)).is_some() {
            continue;
        }
        let mut route = Route {
            // A rendition's canonical address IS its strong address: no rule
            // minted it, the content store did (I11's reading, one transform
            // along).
            strong_url: Some(address.clone()),
            inputs: inputs.clone(),
            rendition: Some(*rendition),
            ..Route::new(address.clone(), RouteKind::Object)
        };
        for (name, value) in &forced {
            route.fields.insert(name.clone(), value.clone());
        }
        db.routes.push(route);
    }

    // The citing edges. A citation writes the baseurl-bearing URL while an
    // output's key does not carry one, so both spellings are indexed — the
    // same seam `Row.url` has, answered here rather than left.
    let mut addr_of: HashMap<&str, &String> = HashMap::new();
    for t in thumbs.values() {
        addr_of.insert(t.address.as_str(), &t.address);
        addr_of.insert(t.url.as_str(), &t.address);
    }
    let mut found: Vec<(grackle_db::Key, Vec<grackle_db::Key>, Vec<grackle_db::Key>)> = Vec::new();
    for (url, urls) in cited {
        let mut facts: Vec<grackle_db::Key> = Vec::new();
        let mut content: Vec<grackle_db::Key> = Vec::new();
        for u in urls {
            let Some(address) = addr_of.get(u.as_str()) else {
                continue;
            };
            let key = grackle_db::Key::new(address);
            if let Some((_, inputs)) = by_address.get(*address) {
                content.extend(inputs.iter().cloned());
            }
            facts.push(key);
        }
        if facts.is_empty() {
            continue;
        }
        found.push((grackle_db::Key::new(url), facts, content));
    }
    for (route, facts, content) in found {
        let Some(r) = db.routes.get_mut(&route) else {
            continue;
        };
        r.route_members.extend(facts);
        r.route_members.sort();
        r.route_members.dedup();
        r.inputs.extend(content);
        r.inputs.sort();
        r.inputs.dedup();
    }
}

/// §4 on-demand: publish a row because something referenced it.
///
/// Runs after the write pass, because the references live in FINISHED output.
/// A body alone is not enough — `{% image %}` expands to
/// `<a href='/assets/…'>` so an original is cited by markup that does not
/// exist at load time, and the shell's favicons and stylesheet link are cited
/// by chrome that no body contains.
///
/// **A fixpoint, not a pass.** Materializing a static HTML file or a
/// stylesheet can introduce references of its own. No iteration bound is
/// needed: each round only ever adds rows from a finite set and a row
/// materializes once, so the loop is monotone and terminates structurally.
/// `cited` is the seed AND the output: the caller scanned every finished
/// document once (`citation_map`), this pass reads that as its frontier and
/// appends an entry for each file it materializes, and IO.md §2's
/// `join_citations` then reads the whole of it. One scan of the output serves
/// both consumers, which is what keeps the join's citation edges free.
///
/// **A pull along the graph's edges** (IO.md §5, I10). A citation is a URL and
/// `db.by_url` is the inputs database's address index, so resolving one *is*
/// walking a content edge to the input at its far end — and the test for "have
/// I materialized this already" is the join's own `output` column rather than
/// a private map of pending URLs. That is the whole rewiring, and it is a
/// deletion: this pass used to key a second index off `on_demand && url`, and
/// two indexes of one fact are two things that can disagree. The behaviour is
/// identical by construction — `by_url` holds exactly the rows that carry a
/// URL, and `output` is `None` for an on-demand row until this line sets it.
pub(crate) fn materialize_referenced(
    db: &mut SiteDb,
    out_map: &mut SiteOutput,
    site_url: &str,
    cited: &mut Vec<(String, Vec<String>)>,
) -> Result<usize> {
    // Nothing to pull: no input publishes lazily, so no edge this pass walks
    // can end at an unminted output. Two shapes qualify — an `on_demand` row,
    // whose RULE deferred its route, and an embed-addressed row (IO.md §4a),
    // which has no route to defer and publishes at its strong address.
    if !db.rows.iter().any(|p| {
        p.output.is_none() && (p.strong_url.is_some() || (p.on_demand && !p.url.is_empty()))
    }) {
        return Ok(0);
    }
    // Rung 0 reaches the outputs minted here too (IO.md I10, closing E1's
    // recorded hole): minting is the graph event, so the seam that mints
    // applies the profile's forced fields rather than leaving a route the
    // profile never saw.
    let forced: Vec<(String, grackle_db::filter::Value)> = db
        .forced_fields
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let mut frontier: Vec<String> = cited.iter().flat_map(|(_, c)| c.iter().cloned()).collect();

    let mut made = 0usize;
    while !frontier.is_empty() {
        let mut next: Vec<String> = Vec::new();
        for url in std::mem::take(&mut frontier) {
            // The edge's far end: which INPUT does this citation name?
            //
            // TWO address indexes, because an output has two address slots
            // (IO.md §4a). `by_url` holds CANONICAL row URLs only, so a
            // `/static/{hash}` citation resolves to nothing there — and review
            // I-D named exactly that as the hole: the pull would never publish
            // the bytes, and `join_citations` below would silently drop the
            // asset edge out of the embedding page's `inputs`. `by_strong` is
            // the other half, and it is a MULTI index because identical bytes
            // legitimately share one address.
            for key in resolve_citation(db, &url) {
                let Some(row) = db.rows.get(&key) else {
                    continue;
                };
                // An input that already lands is not the pull's to mint.
                if row.output.is_some() {
                    continue;
                }
                // Where this input publishes, and whether it publishes lazily
                // at all: its strong address if the policy gave it one, else
                // its own URL — and then only if its rule deferred the route.
                let at = match &row.strong_url {
                    Some(s) => s.clone(),
                    None if row.on_demand => row.url.clone(),
                    None => continue, // lands eagerly, or never
                };
                // The twin: another input with the same bytes already
                // published at this address, so there is ONE artifact and this
                // row is its second input. Not a collision — dedupe, which is
                // what a content address is for — so the edge joins the
                // existing output instead of minting a second one over it.
                let at_key = grackle_db::Key::new(&at);
                if let Some(existing) = db.routes.get_mut(&at_key) {
                    existing.inputs.push(key.clone());
                    existing.inputs.sort();
                    existing.inputs.dedup();
                    if let Some(row) = db.rows.get_mut(&key) {
                        row.output = Some(at_key);
                    }
                    continue;
                }
                let path = row.path.clone();
                let strong = row.strong_url.clone();
                let front_mattered = row.front_mattered;
                let bytes = std::fs::read(&path)
                    .with_context(|| format!("on-demand publish: reading {}", path.display()))?;
                // A materialized text file can cite more — and those citations
                // are edges like any other, so they join the map rather than
                // only the frontier.
                if let Ok(text) = std::str::from_utf8(&bytes) {
                    let mine = cited_urls(text, &at, site_url);
                    next.extend(mine.iter().cloned());
                    if !mine.is_empty() {
                        cited.push((at.clone(), mine));
                    }
                }
                let mut route = Route {
                    row: Some(key.clone()),
                    source: Some(path),
                    front_mattered,
                    // IO.md §4a's second address slot on the output side. For
                    // an output the policy minted this equals `url`: the hash
                    // address is where the artifact landed, because no rule
                    // gave it another one.
                    strong_url: strong,
                    // The content edge, minted with the output it points at.
                    inputs: vec![key.clone()],
                    ..Route::new(at.clone(), RouteKind::Object)
                };
                // Rung 0 reaches BOTH shapes this seam mints (IO.md I10, the
                // law at load.rs's `force_route_fields` call site): minting an
                // output is the graph event, so every minting seam applies
                // `SiteDb::forced_fields`. I11 adds a shape to this seam, not
                // a seam — which is the cheapest possible way to stay inside
                // the law, and the reason the strong mint was folded into this
                // loop rather than written beside it.
                for (name, value) in &forced {
                    route.fields.insert(name.clone(), value.clone());
                }
                // IO.md §2, the pull model made literal: a lazily-published
                // row's `output` is `None` for the whole of the build's
                // queryable life and becomes `Some` exactly here — the moment
                // a reference materialized it. "Bare `output` is truthy iff
                // the row lands anywhere" is then true at every instant rather
                // than true of a plan; what it costs is that a filter, which
                // runs upstream of this pass, always sees the unreferenced
                // answer.
                if let Some(row) = db.rows.get_mut(&key) {
                    row.output = Some(route.id.clone());
                }
                db.routes.push(route);
                out_map.insert(at, bytes);
                made += 1;
            }
        }
        frontier = next;
    }
    Ok(made)
}

/// Internal URLs a document cites, via `href`, `src` or CSS `url(...)`,
/// resolved against the URL the document was published at.
///
/// One scanner for both consumers — the backlink graph and on-demand
/// publishing — because they ask the same question. Relative citations are
/// the common case (§6a: a page bundle keeps its screenshots beside it).
/// Citations only — the forward links a human wrote, excluding a listing's
/// spliced `{% view %}` arrangement (§6g Problem 2). The two relation
/// consumers (backlinks and `links_to`) read this; on-demand publishing reads
/// `cited_urls` directly, because an image referenced only by an arrangement
/// still has to be published.
pub(crate) fn cited_urls_cited(text: &str, base_url: &str, site_url: &str) -> Vec<String> {
    cited_urls(&strip_spliced_views(text), base_url, site_url)
}

/// Blank out the innards of every spliced `{% view %}` region so a listing's
/// arrangement links do not read as citations (§6g Problem 2: "Linked from:
/// Home"). The splice is fenced by HTML comment markers the view splicer
/// emits; everything between a matched pair is replaced by spaces (offsets
/// preserved, so nothing else shifts).
pub(crate) fn strip_spliced_views(text: &str) -> String {
    const OPEN: &str = "<!--grackle:view-->";
    const CLOSE: &str = "<!--/grackle:view-->";
    if !text.contains(OPEN) {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(o) = rest.find(OPEN) {
        out.push_str(&rest[..o]);
        let after = &rest[o..];
        match after.find(CLOSE) {
            Some(c) => {
                // Keep the markers (harmless), drop what they wrap.
                out.push_str(OPEN);
                for _ in 0..(c - OPEN.len()) {
                    out.push(' ');
                }
                out.push_str(CLOSE);
                rest = &after[c + CLOSE.len()..];
            }
            None => {
                out.push_str(after);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

pub(crate) fn cited_urls(text: &str, base_url: &str, site_url: &str) -> Vec<String> {
    let dir = match base_url.rfind('/') {
        Some(i) => &base_url[..=i],
        None => "/",
    };
    let mut out = Vec::new();
    let mut push = |u: &str| {
        let u = u.split('#').next().unwrap_or(u);
        let u = u.split('?').next().unwrap_or(u);
        // Our own absolute form is internal; anyone else's is not.
        let u = u.strip_prefix(site_url).unwrap_or(u);
        if u.is_empty() || u.starts_with("//") || u.contains("://") || u.starts_with("data:") {
            return;
        }
        let abs = if u.starts_with('/') {
            u.to_string()
        } else {
            format!("{dir}{u}")
        };
        // Collapse `.` and `..` so `../img/a.png` names the same URL the
        // route map does. The trailing slash survives: a route URL carries
        // one and the backlink graph matches on the whole string.
        let trailing = abs.ends_with('/');
        let mut segs: Vec<&str> = Vec::new();
        for seg in abs.split('/') {
            match seg {
                "" | "." => {}
                ".." => {
                    segs.pop();
                }
                s => segs.push(s),
            }
        }
        let joined = segs.join("/");
        out.push(match (joined.is_empty(), trailing) {
            (true, _) => "/".to_string(),
            (false, true) => format!("/{joined}/"),
            (false, false) => format!("/{joined}"),
        });
    };
    for pat in ["href=\"", "href='", "src=\"", "src='"] {
        let quote = pat.chars().last().unwrap();
        let mut rest = text;
        while let Some(i) = rest.find(pat) {
            let after = &rest[i + pat.len()..];
            let Some(end) = after.find(quote) else { break };
            push(&after[..end]);
            rest = &after[end..];
        }
    }
    let mut rest = text;
    while let Some(i) = rest.find("url(") {
        let after = &rest[i + 4..];
        let Some(end) = after.find(')') else { break };
        push(after[..end].trim().trim_matches(['"', '\'']));
        rest = &after[end..];
    }
    out
}

#[cfg(test)]
mod cited_url_tests {
    use super::{cited_urls, cited_urls_cited};

    /// §6g Problem 2: an arrangement's links are not citations. The two
    /// clients of one scanner diverge — the backlink/relations view skips a
    /// spliced `{% view %}` region, on-demand publishing keeps it, so an image
    /// only an arrangement references still gets published.
    #[test]
    fn a_spliced_view_is_not_a_citation_but_still_publishes() {
        let html = r#"<a href="/blog/real/">real</a>
            <!--grackle:view--><a href="/blog/listed/">listed</a><img src="/hero.png"><!--/grackle:view-->"#;
        let cited = cited_urls_cited(html, "/", "https://grack.com");
        assert!(cited.contains(&"/blog/real/".to_string()), "{cited:?}");
        assert!(
            !cited.contains(&"/blog/listed/".to_string()),
            "an arrangement link must not count as a citation: {cited:?}"
        );
        // Publishing still sees everything inside the splice.
        let all = cited_urls(html, "/", "https://grack.com");
        assert!(all.contains(&"/blog/listed/".to_string()), "{all:?}");
        assert!(all.contains(&"/hero.png".to_string()), "{all:?}");
    }

    /// The bug this exists to stop, made twice in one session: a scanner that
    /// only accepts root-relative hrefs misses most of a corpus organised as
    /// page bundles (§6a). Measured when it happened: 572 of 838 assets went
    /// unpublished, and the URL-parity check is what caught it.
    #[test]
    fn a_relative_citation_resolves_against_the_citing_url() {
        let refs = cited_urls(
            r#"<img src="screen1.png"><a href="../img/out.png">x</a>"#,
            "/code/legacy/romtool/",
            "https://grack.com",
        );
        assert!(
            refs.contains(&"/code/legacy/romtool/screen1.png".to_string()),
            "{refs:?}"
        );
        assert!(
            refs.contains(&"/code/legacy/img/out.png".to_string()),
            "{refs:?}"
        );
    }

    /// Our own absolute form is internal, anyone else's is not, and a
    /// fragment or query is not part of the target.
    #[test]
    fn site_absolute_is_internal_and_foreign_absolute_is_not() {
        let mut got = cited_urls(
            r##"<a href="/blog/x/">x</a> <a href='/a.png'>i</a>
                <a href="https://grack.com/blog/y/#frag">abs</a>
                <a href="https://elsewhere.com/z">ext</a>
                <a href="//cdn.example/w">rel</a> <a href="#top">frag</a>"##,
            "/page/",
            "https://grack.com",
        );
        got.sort();
        assert_eq!(got, vec!["/a.png", "/blog/x/", "/blog/y/"]);
    }

    #[test]
    fn root_relative_and_css_url_and_externals() {
        let refs = cited_urls(
            r#"<img src="/a/b.png">@font-face{src:url('/css/f.woff2')}
               <a href="https://e.com/x.png">e</a><img src="//cdn/y.png">"#,
            "/page/",
            "https://grack.com",
        );
        assert!(refs.contains(&"/a/b.png".to_string()), "{refs:?}");
        assert!(refs.contains(&"/css/f.woff2".to_string()), "{refs:?}");
        assert!(
            !refs
                .iter()
                .any(|r| r.contains("e.com") || r.contains("cdn")),
            "external citations must not be treated as ours: {refs:?}"
        );
    }
}

#[allow(clippy::too_many_arguments)]
/// Search index and CSS compilation.
pub(crate) fn search_and_css(
    cfg: &Config,
    db: &SiteDb,
    bodies: &HashMap<&grackle_db::Key, Doc>,
    page_bodies: &HashMap<String, PageBody>,
    themes: &crate::theme::Themes,
    root: &Path,
    theme_dir: &Path,
    linkspace: &crate::links::LinkSpace,
    out_map: &mut SiteOutput,
    stats: &mut Stats,
) -> Result<Vec<String>> {
    crate::shells::search::search_pass(cfg, db, bodies, page_bodies, out_map, stats)?;
    let mut warnings = Vec::new();
    for w in linkspace.take_warnings() {
        eprintln!("grackle: {w}");
        warnings.push(w);
    }
    let overlay = crate::shells::css::site_overlay(root, stats);
    crate::shells::css::css_pass(
        theme_dir,
        themes.get(None)?.head_style(),
        "/css/main.css",
        overlay.as_deref(),
        out_map,
        stats,
    )?;
    for name in themes.names().filter(|n| *n != "default") {
        crate::shells::css::css_pass(
            &root.join("themes").join(name),
            themes.get(Some(name))?.head_style(),
            &format!("/css/{name}.css"),
            overlay.as_deref(),
            out_map,
            stats,
        )?;
    }
    Ok(warnings)
}

/// Citations, on-demand publish, rendition joins, strip splice markers.
pub(crate) fn citations(
    cfg: &Config,
    db: &mut SiteDb,
    thumbs: &crate::thumbs::Renditions,
    out_map: &mut SiteOutput,
    stats: &mut Stats,
) -> Result<()> {
    let mut cited = citation_map(out_map, &cfg.site.url);
    stats.on_demand = materialize_referenced(db, out_map, &cfg.site.url, &mut cited)?;
    join_citations(db, &cited);
    join_renditions(db, thumbs, &cited);
    strip_view_markers(out_map);
    Ok(())
}

//! `[profiles.NAME.force]`, rung 0, on both surfaces.
//!
//! This is a whole site rendered twice, so it belongs with the fixtures by
//! `fixtures.rs`'s own line ("if the subject is *a site*, it belongs here").
//! It cannot BE a fixture: the harness builds every fixture with
//! `Config::load` and passes no `--profile` anywhere, which is exactly the
//! property `profile-unknown-view` exists to assert. A projection needs the
//! flag, so it needs its own harness.
//!
//! What is asserted is the rendered bytes rather than a field on a `Route`,
//! because the failure this guards is a rendered one: a listing page that does
//! not say `noindex` while every document under it does, in a projection whose
//! whole purpose is to stay out of search indexes.

use std::path::{Path, PathBuf};

mod support;

/// A site with one post, one listing, and a profile that forces `noindex`.
///
/// The post declares `noindex: false` in its front matter, deliberately, and
/// it is the whole of the rung-0 statement: front matter is rung 1 and wins
/// against every other writer in the system, and it loses to this.
fn site(who: &str) -> PathBuf {
    let files = [
        (
            "grackle.toml",
            "[site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\n\
             [profiles.drafts.force]\nnoindex = true\n",
        ),
        (
            "_posts/2020-01-01-hello.md",
            "---\ntitle: Hello\nnoindex: false\n---\n\nProse.\n",
        ),
    ];
    support::site("force", who, &files)
}

/// Build the site, and hand back (post page, `/blog/` listing) as text.
fn build(dir: &Path, profile: Option<&str>) -> (String, String) {
    let (out, _) = support::render_profile(dir, profile);
    let get = |url: &str| {
        String::from_utf8(
            out.get(url)
                .unwrap_or_else(|| panic!("no route at {url} — routes: {:?}", out.keys()))
                .clone(),
        )
        .expect("html is utf-8")
    };
    (get("/blog/2020/01/01/hello/"), get("/blog/"))
}

const ROBOTS: &str = r#"<meta name="robots" content="noindex,follow">"#;

/// The item, both halves, in one build.
///
/// The base's `robots = 'noindex ? "noindex,follow" : ""'` is what emits the
/// tag, and it is evaluated against the ROW on a document and against the
/// route on a listing, so a force that reached only rows would leave `/blog/`
/// saying nothing at all, which is the sitemap leak exists to close. The
/// two assertions below are the two halves, and each fails alone.
///
/// Mutation-checked in both directions, each restored:
///
/// - delete the `schema::force` calls in `load.rs` (the row half) and the
///   document assertion fails, the post's own `noindex: false` stands and it
///   ships indexable inside a noindexed projection;
/// - delete the `force_route_fields` call (the route half) and the listing
///   assertion fails, with `/blog/` carrying no robots meta at all.
#[test]
fn a_forced_field_reaches_documents_and_listings() {
    let dir = site("both");
    let (post, listing) = build(&dir, Some("drafts"));
    assert!(
        post.contains(ROBOTS),
        "rung 0 beats the row's own `noindex: false`:\n{post}"
    );
    assert!(
        listing.contains(ROBOTS),
        "a listing has no row — the force must reach its ROUTE:\n{listing}"
    );
}

/// The control, and the proof that the two assertions above are about the
/// profile rather than about the base: the same site, same bytes, no `--profile`.
///
/// `noindex: false` is what the post wrote and what it keeps; the listing
/// never had a `noindex` to begin with, so its expression comes out empty and
/// rule 2 drops the tag.
#[test]
fn without_the_profile_the_row_keeps_its_own_answer() {
    let dir = site("control");
    let (post, listing) = build(&dir, None);
    assert!(!post.contains("robots"), "{post}");
    assert!(!listing.contains("robots"), "{listing}");
}

// ---------------------------------------------------------------------------
// Rung 0 is above every reader, selection as well as surface.
// ---------------------------------------------------------------------------

/// The same force, read by two *filters* instead of by two head expressions.
///
/// `row_probe` filters the ROW pool (`from = "published"`, so its clause
/// conjoins along the `from` chain); `pool_probe` filters the route pool (no
/// `from` at all under a fold shell, the sitemap's own shape). Both ask `!noindex`, and under a
/// profile that forces `noindex = true` both must come out empty: a profile
/// changes which rows the views admit, and rung 0 is not exempt from
/// that because it is the highest rung, it is *especially* not exempt.
fn pools_site(who: &str) -> PathBuf {
    let files = [
        (
            "grackle.toml",
            "[site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\n\
             [profiles.drafts.force]\nnoindex = true\n\n\
             [routes.row_probe]\npath = \"/row-probe/\"\nfrom = \"published\"\n\
             where = \"!noindex\"\nlayout = \"card\"\ntitle = \"Row probe\"\n\n\
             [routes.pool_probe]\npath = \"/pool-probe.xml\"\n\
             shell = \"sitemap\"\nwhere = '!noindex && (dir || ext == \"html\")'\n",
        ),
        (
            "_posts/2020-01-01-hello.md",
            "---\ntitle: Hello\n---\n\nProse.\n",
        ),
    ];
    support::site("force-pools", who, &files)
}

/// Build `pools_site` and hand back (row-pool listing, route-pool sitemap).
fn build_pools(dir: &Path, profile: Option<&str>) -> (String, String) {
    let (out, _) = support::render_profile(dir, profile);
    let get = |url: &str| {
        String::from_utf8(
            out.get(url)
                .unwrap_or_else(|| panic!("no route at {url} — routes: {:?}", out.keys()))
                .clone(),
        )
        .expect("html is utf-8")
    };
    (get("/row-probe/"), get("/pool-probe.xml"))
}

/// One law, both pools: a `where` that reads a forced field selects by the
/// forced value, whether it ranges over rows or over routes.
///
/// The route half was previously unguarded, and the ordering it
/// depends on is subtle enough to deserve a test: `force_route_fields` runs
/// while the route list is complete, and `resolve_pool_folds`, the engine's
/// *only* `db.routes.select`, runs at the end of `load`, so the route pool
/// is already forced when it is filtered. Nothing said so, and nothing checked it.
///
/// Mutation-checked in both directions, each restored:
///
/// - move the `force_route_fields` call in `load.rs::load` below the
///   `resolve_pool_folds` call and `/pool-probe.xml` lists all three URLs
///   under the profile, the route pool reads unforced routes;
/// - delete the `schema::force` calls (the row half) and `/row-probe/` links
///   the post under the profile, the row pool reads unforced rows.
#[test]
fn a_forced_field_is_read_by_both_pools_filters() {
    let dir = pools_site("under");
    let (rows, routes) = build_pools(&dir, Some("drafts"));
    assert!(
        !rows.contains("Hello"),
        "the row pool must filter on the forced value:\n{rows}"
    );
    assert!(
        !routes.contains("<loc>"),
        "the route pool must filter on the forced value:\n{routes}"
    );
}

/// The control: without the profile nothing is forced, so both filters admit
/// everything they would have admitted anyway. Without this, the test above
/// passes just as well against an engine that selects nothing, ever.
#[test]
fn without_the_profile_both_pools_admit_everything() {
    let dir = pools_site("control");
    let (rows, routes) = build_pools(&dir, None);
    assert!(rows.contains("Hello"), "{rows}");
    assert!(
        routes.matches("<loc>").count() == 4,
        "the home page, /blog/, /row-probe/ and the post — /pool-probe.xml \
         fails the sitemap's own `dir || ext == \"html\"` clause:\n{routes}"
    );
}

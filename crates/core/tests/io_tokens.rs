//! One route-token supplier.
//!
//! Every test here is a whole site loaded once, because the subject is a
//! *seam*: which tokens a rule's route template may spend was decided in two
//! different loaders, and a unit test over either one would pass against an
//! engine where the other never learned. What is asserted is the URL a row
//! landed at, the one observable that both halves of the supply feed.
//!
//! Rendering is not needed and not done: routing is settled at load, which is
//! also what makes the failure cases *load errors* rather than 404s.

use std::path::{Path, PathBuf};

mod support;
use support::load;

/// Write a site and hand back its directory.
fn site(who: &str, files: &[(&str, &str)]) -> PathBuf {
    support::site("io-tokens", who, files)
}

/// Every row's source path and the URL it routed to.
fn routes(dir: &Path) -> Vec<(String, String)> {
    let db = load(dir);
    let mut out: Vec<(String, String)> = db
        .rows
        .iter()
        .map(|r| (r.rel.to_string_lossy().to_string(), r.url.clone()))
        .collect();
    out.sort();
    out
}

/// The load error, for the shapes that must not load at all.
fn load_error(dir: &Path) -> String {
    let cfg =
        grackle_core::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    match grackle_source::load(&cfg) {
        Ok(_) => panic!("the site loaded, and it must not"),
        Err(e) => format!("{e:#}"),
    }
}

fn url_of(routes: &[(String, String)], rel: &str) -> String {
    routes
        .iter()
        .find(|(r, _)| r == rel)
        .unwrap_or_else(|| panic!("no row at {rel} — rows: {routes:?}"))
        .1
        .clone()
}

/// **Path-token example.** A file in a posts scope routed by its path
/// (`_posts/rust/hello.md` -> `/rust/hello/`) while dated rows of the same
/// collection still route by filename.
///
/// Previously the two suppliers were disjoint and this site did not load:
/// *"template `/{dir}/{stem}/` references unknown token {dir}"*, measured
/// against the HEAD binary. Nothing about the file was wrong; the posts loader
/// simply had no path tokens to give.
///
/// Mutation: drop the `path_tokens` call from `RouteTokens::get` and this test
/// fails with that same error, while the dated row below still routes, which
/// is the disjointness, restored.
#[test]
fn a_posts_scope_routes_by_path_tokens() {
    let dir = site(
        "path-tokens",
        &[
            (
                "grackle.toml",
                "[site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\n\
                 [[collections]]\nname = \"posts\"\nsource = \"_posts\"\n\n\
                 [[collections.rules]]\nmatch = \"rust/**\"\nroute = \"/{dir}/{stem}/\"\n",
            ),
            ("_posts/rust/hello.md", "---\ntitle: Hello\n---\n\nProse.\n"),
            (
                "_posts/2020-01-01-dated.md",
                "---\ntitle: Dated\n---\n\nProse.\n",
            ),
        ],
    );
    let r = routes(&dir);
    assert_eq!(url_of(&r, "_posts/rust/hello.md"), "/rust/hello/");
    // The inherited dated rule, untouched, the capability is additive.
    assert_eq!(
        url_of(&r, "_posts/2020-01-01-dated.md"),
        "/blog/2020/01/01/dated/"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// **The token set per rule, in one template.** A route spending a path token
/// and an extractor token together is the statement that there is one supply,
/// not two that alternate: `{dir}` comes from the walk, `{year}` and `{slug}`
/// from the rule's `file`, and they meet in one URL.
///
/// The same site pins the other half of the shape, that the extractor is the
/// rule's: `legacy/**` declares the MM-DD-YYYY form and its row routes by it,
/// while the collection's own list still governs everything else. First writer
/// wins per key, the collection is the default, and the two are visible
/// in one build.
#[test]
fn one_template_spends_both_suppliers_and_the_rule_owns_the_extractor() {
    let dir = site(
        "both-suppliers",
        &[
            (
                "grackle.toml",
                "[site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\n\
                 [[collections]]\nname = \"posts\"\nsource = \"_posts\"\n\
                 file = [\"{date.year}-{date.month}-{date.day}-{slug}\"]\n\n\
                 [[collections.rules]]\nmatch = \"legacy/**\"\n\
                 file = [\"{date.month}-{date.day}-{date.year}-{slug}\"]\n\
                 route = \"/{dir}/{year}/{slug}/\"\n\n\
                 [[collections.rules]]\nmatch = \"rust/**\"\n\
                 route = \"/{dir}/{year}/{slug}/\"\n",
            ),
            (
                "_posts/rust/2020-03-04-hello.md",
                "---\ntitle: Hello\n---\n\nProse.\n",
            ),
            (
                "_posts/legacy/12-06-2014-old.md",
                "---\ntitle: Old\n---\n\nProse.\n",
            ),
            (
                "_posts/2020-01-01-dated.md",
                "---\ntitle: Dated\n---\n\nProse.\n",
            ),
        ],
    );
    let r = routes(&dir);
    assert_eq!(
        url_of(&r, "_posts/rust/2020-03-04-hello.md"),
        "/rust/2020/hello/"
    );
    // The rule's own format read a name the collection's cannot: without it
    // this stem has no date at all, and the load below would be the error.
    assert_eq!(
        url_of(&r, "_posts/legacy/12-06-2014-old.md"),
        "/legacy/2014/old/"
    );
    // …and the collection's list still governs the rows no rule re-declared.
    assert_eq!(
        url_of(&r, "_posts/2020-01-01-dated.md"),
        "/blog/2020/01/01/dated/"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// An extractor reaches any rule that declares `file`. A tree rule naming
/// `file` routes a dated tree page by its filename.
#[test]
fn an_extractor_reaches_a_tree_rule() {
    let dir = site(
        "tree-extractor",
        &[
            (
                "grackle.toml",
                "[site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\n\
                 [[collections]]\nname = \"entries\"\nsource = \".\"\n\n\
                 [[collections.rules]]\nmatch = { pattern = \"notes/**\", front_matter = true }\n\
                 file = [\"{date.year}-{date.month}-{date.day}-{slug}\"]\n\
                 route = \"/{year}/{slug}/\"\n",
            ),
            (
                "notes/2020-01-02-hello.md",
                "---\ntitle: Hello\n---\n\nProse.\n",
            ),
            ("about.md", "---\ntitle: About\n---\n\nProse.\n"),
        ],
    );
    let r = routes(&dir);
    assert_eq!(url_of(&r, "notes/2020-01-02-hello.md"), "/2020/hello/");
    // The row wears what the extractor found, not just the URL: `slug` is the
    // format's capture rather than the whole stem.
    let cfg =
        grackle_core::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    let db = grackle_source::load(&cfg).expect("the site loads");
    let row = db
        .rows
        .iter()
        .find(|r| r.rel.to_string_lossy() == "notes/2020-01-02-hello.md")
        .expect("the row loaded");
    assert_eq!(row.slug, "hello");
    assert_eq!(
        row.as_date("date").map(|d| d.to_string()).as_deref(),
        Some("2020-01-02")
    );
    // The base's own rules still route the page that no rule of the site's
    // covers, by the path tokens they have always spent.
    assert_eq!(url_of(&r, "about.md"), "/about/");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A row with no date under a template that spends one is a load error
/// naming the file and the rule. The check rides the supplier, so it covers
/// a tree rule and an objects rule the same way.
///
/// Mutation (measured, and the reason this asserts the sentence rather than
/// the failure): delete the `check` call in `render_all` and the load still
/// fails, `template::render` refuses an unresolved token, but by the generic
/// sentence, *"template `/blog/{year}/{slug}/` references unknown token
/// {year}"*, which sends its reader to look for a typo in a template that is
/// perfectly well spelled. The check buys the diagnosis, not the refusal.
#[test]
fn an_undated_row_under_a_dated_template_names_the_file_and_the_rule() {
    let dir = site(
        "undated",
        &[
            (
                "grackle.toml",
                "[site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\n\
                 [[collections]]\nname = \"posts\"\nsource = \"_posts\"\n\
                 file = [\"{date.year}-{date.month}-{date.day}-{slug}\"]\n\n\
                 [[collections.rules]]\nmatch = \"notes/**\"\n\
                 route = \"/blog/{year}/{slug}/\"\n",
            ),
            ("_posts/notes/undated.md", "---\ntitle: U\n---\n\nProse.\n"),
        ],
    );
    let e = load_error(&dir);
    assert!(e.contains("undated.md"), "names the file: {e}");
    assert!(e.contains("match = \"notes/**\""), "names the rule: {e}");
    assert!(e.contains("has no date"), "says why: {e}");
    assert!(e.contains("{year}"), "names the token: {e}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The general form of the same refusal: a token nothing supplies. The
/// sentence lists what a route MAY spend, because the reader's next move is to
/// pick one, and the list is the supplier's own, so it cannot drift.
#[test]
fn a_token_nothing_supplies_lists_what_a_route_may_spend() {
    let dir = site(
        "unknown-token",
        &[
            (
                "grackle.toml",
                "[site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\n\
                 [[collections]]\nname = \"entries\"\nsource = \".\"\n\n\
                 [[collections.rules]]\nmatch = { pattern = \"*.md\", front_matter = true }\n\
                 route = \"/{author}/{stem}/\"\n",
            ),
            ("about.md", "---\ntitle: About\n---\n\nProse.\n"),
        ],
    );
    let e = load_error(&dir);
    assert!(e.contains("about.md"), "names the file: {e}");
    assert!(e.contains("match = \"*.md\""), "names the rule: {e}");
    assert!(e.contains("{author}"), "names the token: {e}");
    assert!(
        e.contains("path, dir, stem, name, ext"),
        "lists the path tokens: {e}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// **A posts collection may declare no extractor at all**, which retires the
/// old refusal (*"kind=posts but no file"*): the
/// requirement was never about the collection, it was about whether a
/// template spends a date, and that is asked per row now.
#[test]
fn a_posts_scope_needs_no_extractor_when_no_route_spends_a_date() {
    let dir = site(
        "no-formats",
        &[
            (
                "grackle.toml",
                "extends = \"none\"\n\n\
                 [site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\n\
                 [[collections]]\nname = \"posts\"\nsource = \"_posts\"\n\n\
                 [[collections.rules]]\nmatch = \"**\"\nroute = \"/notes/{stem}/\"\n\n\
                 [[collections]]\nname = \"entries\"\nsource = \".\"\n\n\
                 [[collections.rules]]\nmatch = \"**/*\"\nroute = \"/{path}\"\n",
            ),
            ("_posts/hello.md", "---\ntitle: Hello\n---\n\nProse.\n"),
        ],
    );
    let r = routes(&dir);
    assert_eq!(url_of(&r, "_posts/hello.md"), "/notes/hello/");
    let _ = std::fs::remove_dir_all(&dir);
}

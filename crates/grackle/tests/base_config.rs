//! The base config's falsifier (DESIGN.md §4d).
//!
//! `examples/raw` is the base config spelled out under `extends = "none"`, and
//! `examples/minimal` is an empty file over the same content tree. If the two
//! stop agreeing, either the compiled base changed without its printed copy or
//! the merge is not doing what the printed copy says — and both failures are
//! invisible by inspection, because the whole point of the base is that you
//! never see it.

use std::path::{Path, PathBuf};

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn urls(config: &Path) -> Vec<String> {
    let cfg = grackle_source::config::Config::load(config)
        .unwrap_or_else(|e| panic!("loading {}: {e:#}", config.display()));
    let db = grackle_source::load(&cfg)
        .unwrap_or_else(|e| panic!("loading the db for {}: {e:#}", config.display()));
    let mut u: Vec<String> = db.routes.iter().map(|r| r.url.clone()).collect();
    u.sort();
    u
}

/// The empty config and the spelled-out one produce the same site.
#[test]
fn an_empty_config_and_the_printed_base_agree() {
    let inherited = urls(&examples().join("minimal/grackle.toml"));
    let spelled_out = urls(&examples().join("raw/grackle.toml"));
    assert_eq!(
        inherited, spelled_out,
        "examples/raw has drifted from the compiled base config"
    );
}

/// The claim that makes §4d worth having: zero lines of config is a whole
/// site. Asserted by URL rather than by count so that it fails when a route
/// silently stops materializing, not only when the base shrinks.
#[test]
fn zero_lines_of_config_is_a_whole_site() {
    let u = urls(&examples().join("minimal/grackle.toml"));
    for want in [
        "/",                       // the homepage listing
        "/about/",                 // a page, pretty URL
        "/blog/",                  // the archive
        "/blog/2026/01/01/hello/", // a post
        "/atom.xml",               // the feed
        "/sitemap.xml",            // the sitemap
    ] {
        assert!(u.iter().any(|x| x == want), "{want} missing from {u:?}");
    }
}

/// The base's second rule: it may not mint a URL the author did not ask for.
/// A site with no `_posts/` never asked for an empty `/blog/` or a feed with
/// no entries, so the inherited routes with nothing to show stand down — while
/// `/about/` and the sitemap, which have something to say, do not.
#[test]
fn an_inherited_route_with_nothing_to_show_does_not_materialize() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-posts");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("grackle.toml"), "").unwrap();
    std::fs::write(dir.join("about.md"), "---\ntitle: About\n---\n\nHello.\n").unwrap();

    let u = urls(&dir.join("grackle.toml"));
    assert!(u.iter().any(|x| x == "/about/"), "{u:?}");
    assert!(u.iter().any(|x| x == "/sitemap.xml"), "{u:?}");
    for unwanted in ["/", "/blog/", "/atom.xml"] {
        assert!(
            !u.iter().any(|x| x == unwanted),
            "{unwanted} materialized with nothing in it: {u:?}"
        );
    }
}

/// §4e: `draft` is a declared field, not engine vocabulary. Under the base it
/// is there because `base.toml` declares it; under `extends = "none"` a site
/// that never declared it cannot filter on it, and the error names the knowns
/// instead of the filter quietly matching everything.
#[test]
fn the_flag_family_is_the_sites_vocabulary_not_the_engines() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-flags");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("about.md"), "---\ntitle: About\n---\n\nHi.\n").unwrap();

    let with_base = "[sets.s]\nfrom = \"entries\"\nwhere = \"!draft\"\n";
    std::fs::write(dir.join("grackle.toml"), with_base).unwrap();
    urls(&dir.join("grackle.toml")); // inherits [schema]; parses.

    // A second directory, with no content: the filter is type-checked at load
    // whether or not any row exists, so the assertion is about vocabulary.
    let bare = Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-flags-bare");
    std::fs::create_dir_all(&bare).unwrap();
    let alone = format!(
        "extends = \"none\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n\
         [[collections]]\nkind = \"tree\"\nsource = \".\"\n\
         [[collections.rules]]\nmatch = \"**/*\"\nroute = \"/{{path}}\"\n{with_base}"
    );
    std::fs::write(bare.join("grackle.toml"), &alone).unwrap();
    let cfg = grackle_source::config::Config::load(&bare.join("grackle.toml")).unwrap();
    let e = grackle_source::load(&cfg).unwrap_err();
    let e = format!("{e:#}");
    assert!(e.contains("unknown field `draft`"), "{e}");
}

/// `extends = "none"` means none: the stock setup is the site's own file and
/// nothing else. Mutation-checked when written by deleting the key.
#[test]
fn extends_none_inherits_nothing() {
    let cfg = grackle_source::config::Config::from_toml(
        "extends = \"none\"\n[site]\nurl = \"u\"\ntitle = \"t\"\nauthor = \"a\"\n",
    )
    .expect("a site may decline the base entirely");
    assert!(cfg.collections.is_empty(), "{:?}", cfg.collections.keys());
    assert!(cfg.views.is_empty(), "{:?}", cfg.views.keys());
    assert!(cfg.markers.is_empty(), "{:?}", cfg.markers.keys());
}

// ------------------------------------------------- `config --effective` (B3)
//
// No golden file, deliberately. `examples/raw` is already the printed base and
// it is a document a person edits and argues with; a machine-generated second
// copy would have to be re-blessed on every `base.toml` edit, and a golden
// nobody reads is a test that asserts the code does what the code does
// (fixtures.rs says this out loud). What follows asserts the things a golden
// would have been consulted FOR, and none of them churn: the text is TOML, and
// the two example sites that have an absolute answer about provenance give it.

/// Every config in the repo prints, and prints TOML. Comments are TOML's own,
/// so "valid with the comments stripped" needs no stripping — the parser does
/// it, which is also the only definition of stripping that cannot be wrong.
#[test]
fn every_sites_effective_config_parses_back() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for rel in [
        "grackle.toml", // grack.com itself
        "theme-preview/grackle.toml",
        "examples/minimal/grackle.toml",
        "examples/raw/grackle.toml",
        "examples/field-notes/grackle.toml",
    ] {
        let path = repo.join(rel);
        let printed = grackle_source::config::Config::effective(&path, None)
            .unwrap_or_else(|e| panic!("{rel}: {e:#}"));
        let back: toml::Value = toml::from_str(&printed)
            .unwrap_or_else(|e| panic!("{rel} printed something that is not TOML: {e}\n{printed}"));
        let t = back.as_table().expect("a config is a table");
        assert!(t.contains_key("site"), "{rel} lost [site]");
        assert!(t.contains_key("collections"), "{rel} lost the collections");
    }
}

/// The claim §4d is built on, seen instead of inferred: an empty
/// `grackle.toml` is a whole config, and every line of it says `base` —
/// except the three keys neither file writes, which say `default`.
///
/// This is the golden test's job without the golden's churn: it fails if the
/// merge starts attributing the base's own values to the site, and it does not
/// care what the base config happens to contain today.
#[test]
fn the_empty_sites_effective_config_is_entirely_inherited() {
    let path = examples().join("minimal/grackle.toml");
    let printed = grackle_source::config::Config::effective(&path, None).unwrap();
    let body: Vec<&str> = printed
        .lines()
        .skip_while(|l| l.starts_with('#') || l.is_empty())
        .collect();
    assert!(body.len() > 50, "suspiciously short:\n{printed}");
    for line in &body {
        let Some((_, comment)) = line.split_once("# ") else {
            continue;
        };
        assert!(
            comment.starts_with("base") || comment.starts_with("default"),
            "the empty site wrote something: {line}"
        );
    }
    assert!(
        body.iter().any(|l| l.contains("# default")),
        "the defaulted keys are missing:\n{printed}"
    );
}

/// And its mirror: `examples/raw` inherits nothing, so nothing may be
/// attributed to the base. The pair is what proves the comments track the
/// merge rather than the shape of the file.
#[test]
fn the_uninheriting_sites_effective_config_is_entirely_its_own() {
    let path = examples().join("raw/grackle.toml");
    let printed = grackle_source::config::Config::effective(&path, None).unwrap();
    let body: Vec<&str> = printed
        .lines()
        .skip_while(|l| l.starts_with('#') || l.is_empty())
        .collect();
    for line in &body {
        let Some((_, comment)) = line.split_once("# ") else {
            continue;
        };
        assert!(
            comment.starts_with("site") || comment.starts_with("default"),
            "extends = \"none\" inherited something: {line}"
        );
    }
}

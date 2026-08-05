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
         [[collections]]\nsource = \".\"\n\
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
        "examples/theme-preview/grackle.toml",
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

/// Every top-level field `Config` gives a value to when NEITHER file writes
/// one is in minimal's effective config — the guard `engine_defaults()`'s doc
/// comment has promised since B3 and did not have (batch review
/// 2 finding 2: deleting `("extends", …)` passed the whole suite).
///
/// `engine_defaults()` is the one hand-maintained list in `--effective`. It
/// does not COPY the defaults — it calls the very functions each field's
/// `#[serde(default = "…")]` names — but a NEW defaulted field still has to be
/// added to it, and nothing said so out loud. This is what says so: a key with
/// an engine default that nobody adds is a key that prints nowhere, on the one
/// site where "nowhere" is the whole output.
#[test]
fn every_defaulted_scalar_is_printed() {
    let defaulted = defaulted_scalars();
    assert!(
        defaulted.len() >= 3,
        "the extraction found almost nothing — has `Config` moved? {defaulted:?}"
    );
    let path = examples().join("minimal/grackle.toml");
    let printed = grackle_source::config::Config::effective(&path, None).unwrap();
    // Top level only: everything before the first table header, which is
    // where a key of `Config`'s own can appear.
    let top: Vec<&str> = printed
        .lines()
        .take_while(|l| !l.starts_with('['))
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once(" =").map(|(k, _)| k.trim()))
        .collect();
    for key in &defaulted {
        assert!(
            top.contains(&key.as_str()),
            "`{key}` has an engine default and the empty site's effective config \
             never mentions it — add it to `engine_defaults()`. Printed:\n{printed}"
        );
    }
}

/// The defaulted set, read off `Config`'s own text rather than restated here.
///
/// `#[serde(default = "…")]` names a function that RETURNS a value, which is
/// exactly the set `engine_defaults()` must call; plain `#[serde(default)]` is
/// an empty container (`[sets]`, `[markers]`, …), which has no value to print
/// and is absent from an effective config on purpose.
fn defaulted_scalars() -> Vec<String> {
    defaulted_scalars_in(include_str!("../../source/src/config/types.rs"))
}

/// The extraction itself, over any source text — which is what lets the
/// spellings it must refuse be exhibited rather than described.
fn defaulted_scalars_in(src: &str) -> Vec<String> {
    let body = src
        .split_once("pub struct Config {")
        .expect("`Config` is declared in crates/source/src/config/types.rs")
        .1
        .split_once("\n}\n")
        .expect("the struct is closed")
        .0;
    let (mut out, mut armed, mut renamed) = (Vec::new(), false, false);
    for line in body.lines().map(str::trim) {
        if line.starts_with("#[serde(default = \"") {
            armed = true;
            // The one-line spelling, `#[serde(default = "…", rename = "…")]`,
            // arms the extraction here, so it must arm the refusal here too —
            // the `else if` below never sees it (batch review 3, finding 7).
            renamed |= line.contains("rename");
        } else if line.starts_with("#[serde(") && line.contains("rename") {
            renamed = true;
        } else if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
            // Doc comments and the other attributes say nothing about defaults.
        } else {
            // A field declaration: `pub name: Type,`.
            if armed {
                assert!(
                    !renamed,
                    "a defaulted field is renamed, and this reads RUST names: {line}"
                );
                let name = line.trim_start_matches("pub ").split(':').next().unwrap();
                out.push(name.to_string());
            }
            (armed, renamed) = (false, false);
        }
    }
    out
}

/// A synthetic `Config`, so the extractor's own arms can be exercised without
/// waiting for someone to write these lines in the real one. `{ATTRS}` is the
/// attribute block under test.
fn synthetic(attrs: &str) -> String {
    format!("pub struct Config {{\n    {attrs}\n    pub extends: String,\n}}\n")
}

/// What the extraction is FOR: a default that names a function is collected at
/// its Rust name; a bare `#[serde(default)]` is an empty container with no
/// value to print; a rename with no default is nobody's business here.
#[test]
fn the_extractor_collects_defaults_that_name_a_function() {
    assert_eq!(
        defaulted_scalars_in(&synthetic("#[serde(default = \"default_extends\")]")),
        vec!["extends".to_string()]
    );
    assert!(defaulted_scalars_in(&synthetic("#[serde(default)]")).is_empty());
    assert!(defaulted_scalars_in(&synthetic("#[serde(rename = \"inherits\")]")).is_empty());
}

/// The refusal, in the spelling that used to evade it: one attribute carrying both keys armed the extraction
/// on the `if` arm, so the `else if` that watches for `rename` never ran and a
/// renamed field would have been collected — and then reported missing from an
/// effective config that prints it under its TOML name.
#[test]
#[should_panic(expected = "a defaulted field is renamed")]
fn a_default_and_a_rename_on_one_line_is_refused() {
    defaulted_scalars_in(&synthetic(
        "#[serde(default = \"default_extends\", rename = \"inherits\")]",
    ));
}

/// Its two-line twin, which the assert already caught — kept beside it so the
/// pair says the refusal is about the FIELD, not about a line.
#[test]
#[should_panic(expected = "a defaulted field is renamed")]
fn a_default_and_a_rename_on_two_lines_is_refused() {
    defaulted_scalars_in(&synthetic(
        "#[serde(default = \"default_extends\")]\n    #[serde(rename = \"inherits\")]",
    ));
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

/// The preamble asserted a projection without ever checking
/// there was one, so `--effective --profile nosuch` explained the effect of a
/// profile that does not exist — while `build --profile nosuch` refuses the
/// same name outright. One lookup against the merged `[profiles]` table.
///
/// It keeps printing, deliberately: the merge below is what was asked for and
/// is unaffected by a name that names nothing — the projection is the part
/// that would not have happened.
///
/// Mutation check: drop the `known.contains` test and the unknown name reads
/// as a projection again, which is the whole finding.
#[test]
fn an_unknown_profile_is_named_in_the_effective_preamble() {
    // grack.com is the only site in the repo that declares a profile.
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../grackle.toml");
    let printed = grackle_source::config::Config::effective(&path, Some("nosuch")).unwrap();
    assert!(
        printed.contains("\"nosuch\" names no profile"),
        "{}",
        &printed[..600.min(printed.len())]
    );
    assert!(
        printed.contains("knowns: dev, drafts"),
        "the knowns include the implicit `dev`: {}",
        &printed[..600.min(printed.len())]
    );
    assert!(
        !printed.contains("Projected through"),
        "it must not also claim a projection"
    );
    // And it printed the config anyway.
    let back: toml::Value = toml::from_str(&printed).expect("still TOML");
    assert!(back.as_table().expect("a table").contains_key("site"));

    // The declared one is projected, and says so.
    let ok = grackle_source::config::Config::effective(&path, Some("drafts")).unwrap();
    assert!(ok.contains("Projected through profile \"drafts\""));
    assert!(!ok.contains("names no profile"));
}

/// `--effective --profile NAME` prints the PROJECTED config, and
/// the overlay is one more writer in the same traced merge — so a key the
/// profile wrote reads `# profile NAME` exactly as a key the site wrote reads
/// `# site`. B3's design, carried to a fourth rung: there is no second
/// traversal and no after-the-fact diff, so the provenance cannot disagree with
/// the projection the build ran.
///
/// It also shows the LAW. `[sets.published]` is a definition, so the comment
/// sits on the header and none of its keys carry one — you never inherit half
/// of one, which is why grack.com's drafts profile restates the set in full.
///
/// Mutation check: delete the `project` call in `effective_toml` and the
/// profile's `where` is missing from the output entirely; give the overlay pass
/// a `far` provenance (`Trace::layer` keeping `Some(Prov::Base)`) and every key
/// the profile did not write is re-labelled `base`, erasing the merge below.
#[test]
fn the_effective_config_shows_the_projection() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../grackle.toml");
    let plain = grackle_source::config::Config::effective(&path, None).unwrap();
    let drafts = grackle_source::config::Config::effective(&path, Some("drafts")).unwrap();
    assert!(!plain.contains("# profile"), "no profile, no such class");

    // The value the projection actually runs on, not a note about one.
    assert!(plain.contains(r#"where = "!draft && !hidden""#));
    assert!(drafts.contains(r#"where = "!hidden""#));
    // Whole definition, one comment, on the header.
    let line = drafts
        .lines()
        .find(|l| l.starts_with("[sets.published]"))
        .expect("the projected set");
    assert!(line.contains("# profile drafts, whole"), "{line}");
    for key in ["from = ", "order_by = "] {
        let line = drafts
            .lines()
            .find(|l| l.trim_start().starts_with(key))
            .expect(key);
        assert!(
            !line.contains('#'),
            "a definition's keys say nothing: {line}"
        );
    }
    // Everything the profile did not write keeps what the merge UNDERNEATH
    // decided — the overlay has no farther writer to attribute, because that
    // question was already answered one layer down.
    let untouched = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|l| l.starts_with("url = ") || l.starts_with("[markers."))
            .map(str::to_string)
            .collect()
    };
    assert_eq!(
        untouched(&plain),
        untouched(&drafts),
        "the overlay wrote none of these, so their provenance is unmoved"
    );
    assert!(untouched(&plain)
        .iter()
        .any(|l| l.contains("# site over base")));
    assert!(
        drafts.contains("# base"),
        "the base rung survives the overlay"
    );
    let legend = "#   # profile drafts  the profile's overlay";
    assert!(drafts.contains(legend), "the legend names the class");
    assert!(!plain.contains(legend), "and only when there is one");

    // Rung 0 is not overlay: it prints under `[profiles]`, where it is written.
    assert!(drafts.contains("[profiles.drafts.force]"));
    assert!(drafts.contains("[profiles.drafts.force] is NOT part of that overlay"));

    // Still TOML, still round-trips.
    toml::from_str::<toml::Value>(&drafts).expect("the projected config parses back");
}

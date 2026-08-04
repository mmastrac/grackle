//! The rendering law (IO.md I7c): **a row renders iff
//! `front_mattered || shell ∈ {html, light_html}`.**
//!
//! The front-matter gate used to BE the answer on the tree side
//! (`rendered: f.has_front_matter`) and a constant on the posts side
//! (`rendered: true`). It is now one clause of a disjunction, and both clauses
//! are load-bearing on the corpus — which is why every test here holds both
//! halves rather than one:
//!
//! - drop the `front_mattered` clause and `examples/field-notes`'
//!   `demos/pane.html` — front-mattered AND `shell: raw` — stops being a
//!   document and byte-copies, shipping its `---` block to the browser;
//! - drop the `shell` clause and grack.com's `_drafts/caret/…`, the corpus's
//!   one blockless `.md`, stops being a page at a URL it has published for
//!   years.
//!
//! Built sites rather than loaded ones: what the law decides is what comes out
//! of the build, and a test that asked the loader for `rendered` would pass
//! against an engine that emitted the wrong bytes anyway.

use std::path::{Path, PathBuf};

mod support;

const HEAD: &str = "extends = \"none\"\n\
     [site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\n\
     [schema]\nshell = { type = \"string\" }\n\n\
     [html.head.property]\n\"og:title\" = 'title'\n\n\
     [[collections]]\nsource = \".\"\n";

/// Write a site and hand back its directory. Per-test directories, because
/// tests that share one temp tree and each clear it race (IR3).
fn site(who: &str, config: &str, files: &[(&str, &str)]) -> PathBuf {
    let mut all: Vec<(&str, &str)> = vec![("grackle.toml", config)];
    all.extend_from_slice(files);
    support::site("io-gate", who, &all)
}

/// The published bytes, and the warnings the load emitted — the two halves a
/// degenerate row is judged on.
fn build(dir: &Path) -> (std::collections::BTreeMap<String, String>, Vec<String>) {
    let cfg =
        grackle_core::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    let mut db = grackle_source::load(&cfg).expect("the site loads");
    let warnings = db.warnings.clone();
    let (out, _) = grackle_core::build::render_site(&cfg, &mut db).expect("the site renders");
    let _ = std::fs::remove_dir_all(dir.join("_cache"));
    let files = out
        .into_iter()
        .map(|(u, b)| (u, String::from_utf8_lossy(&b).into_owned()))
        .collect();
    (files, warnings)
}

/// **The degenerate row.** A blockless `.md` under a rule that defaults
/// `shell = "html"` is not bytes: it renders, at the URL its rule minted, with
/// a title implied from its slug — and the build says so once, on stderr,
/// without failing.
///
/// The `light_html` twin is here because the law reads the document FAMILY,
/// not the word `html`: `shell::DOCUMENT` is the set, and a test naming one
/// member would pass against an engine that had forgotten the other.
///
/// Mutations, each red: delete the `shell` clause of `shell::renders` and both
/// files byte-copy their own markdown at `/notes/blockless-note.md`; delete the
/// `degenerate_warning` push and the two warnings go silent while the pages
/// still ship, which is the softening losing its nudge.
#[test]
fn a_blockless_file_under_a_document_shell_renders_and_warns() {
    let dir = site(
        "degenerate",
        &format!(
            "{HEAD}\n  [[collections.rules]]\n  match = \"notes/*.md\"\n  \
             route = \"/{{stem}}/\"\n  defaults = {{ shell = \"html\" }}\n\n  \
             [[collections.rules]]\n  match = \"light/*.md\"\n  \
             route = \"/{{stem}}/\"\n  defaults = {{ shell = \"light_html\" }}\n\n  \
             [[collections.rules]]\n  match = \"**/*\"\n  route = \"/{{path}}\"\n  \
             defaults = {{ shell = \"raw\" }}\n"
        ),
        &[
            ("notes/blockless-note.md", "Just a paragraph.\n"),
            ("light/thin-note.md", "Thin.\n"),
        ],
    );
    let (out, warnings) = build(&dir);

    // It is a document, not its own source bytes.
    let page = out.get("/blockless-note/").expect("the row rendered");
    assert!(page.starts_with("<!doctype html>"), "{page}");
    assert!(
        page.contains("<title>blockless note</title>"),
        "the slug title is missing: {page}"
    );
    let thin = out.get("/thin-note/").expect("the light row rendered");
    assert!(thin.starts_with("<!doctype html>"), "{thin}");
    assert!(thin.contains("<title>thin note</title>"), "{thin}");
    // …and the raw catch-all did NOT also publish it at its literal path: one
    // row, one route, and the document rule won it.
    assert!(
        !out.contains_key("/notes/blockless-note.md"),
        "byte-copied too"
    );

    assert_eq!(warnings.len(), 2, "{warnings:?}");
    let w = warnings.join("\n");
    assert!(w.contains("notes/blockless-note.md"), "{w}");
    assert!(w.contains("no front-matter block"), "{w}");
    assert!(w.contains("`html` shell"), "{w}");
    assert!(w.contains("\"blockless note\""), "{w}");
    assert!(w.contains("`light_html` shell"), "{w}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// **The control, and the reason the law is a disjunction.**
/// `examples/field-notes`' `demos/pane.html` is front-mattered AND
/// `shell: raw` — an imported document that carries identity (a title, a
/// `hidden` flag) and wants its own bytes on the wire. It is the shape a law
/// spelled "the shell decides" gets wrong, and it gets it wrong LOUDLY: a byte
/// copy of a front-mattered file ships the `---` block to the browser.
///
/// The blockless twin beside it is the other control: identity-less and `raw`
/// is the ordinary byte row, and it earns no warning, because that is the
/// normal case rather than a degenerate one.
///
/// Mutation: drop the `front_mattered` clause of `shell::renders` and
/// `/pane.html` comes out with `---\ntitle: Glass pane\n…` as its first bytes.
#[test]
fn a_front_mattered_raw_row_is_still_a_document() {
    let dir = site(
        "raw-control",
        &format!(
            "{HEAD}\n  [[collections.rules]]\n  match = \"**/*\"\n  \
             route = \"/{{path}}\"\n  defaults = {{ shell = \"raw\" }}\n"
        ),
        &[
            (
                "pane.html",
                "---\ntitle: Glass pane\n---\n<p>An imported artifact.</p>\n",
            ),
            ("plain.html", "<p>No identity here.</p>\n"),
        ],
    );
    let (out, warnings) = build(&dir);

    let pane = out.get("/pane.html").expect("the row published");
    assert!(
        !pane.contains("---"),
        "the front-matter block shipped: {pane}"
    );
    assert!(!pane.contains("title: Glass pane"), "{pane}");
    assert!(pane.contains("An imported artifact."), "{pane}");
    // `raw` is the transparent shell: rendered, then emitted verbatim. No
    // engine skeleton wrapped it.
    assert!(!pane.contains("<!doctype html>"), "{pane}");

    // The byte row beside it, untouched and unremarked.
    assert_eq!(
        out.get("/plain.html").map(String::as_str),
        Some("<p>No identity here.</p>\n")
    );
    assert_eq!(warnings, Vec::<String>::new(), "{warnings:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// **The derivation is pinned.** `slug.replace('-', " ")` and nothing prettier:
/// the posts loader has spelled it this way since before the ledger, and
/// grack.com publishes `<title>why is a cursor called a caret</title>` from it
/// today. Title-casing it, or teaching it about acronyms, moves bytes on a live
/// page — so the exact string is asserted on both surfaces that read a title,
/// and the underscore case says the rule is about hyphens alone.
///
/// Mutation (run on the corpus, not only here): make it prettier — e.g.
/// title-case each word — and grack.com's `/drafts/why-is-a-cursor-called-a-
/// caret/` moves its `<title>` and its `og:title`, and the drafts profile's
/// `/search.bin` moves with them.
#[test]
fn the_implied_title_is_the_slugs_hyphens_and_nothing_else() {
    let dir = site(
        "pin",
        &format!(
            "{HEAD}\n  [[collections.rules]]\n  match = \"**/*.md\"\n  \
             route = \"/{{stem}}/\"\n  defaults = {{ shell = \"html\" }}\n"
        ),
        &[
            ("why-is-a-cursor-called-a-caret.md", "Body.\n"),
            ("an_underscored_name.md", "Body.\n"),
            ("ALL-CAPS-HERE.md", "Body.\n"),
        ],
    );
    let (out, _) = build(&dir);

    let caret = out
        .get("/why-is-a-cursor-called-a-caret/")
        .expect("the row rendered");
    assert!(
        caret.contains("<title>why is a cursor called a caret</title>"),
        "{caret}"
    );
    assert!(
        caret.contains("<meta property=\"og:title\" content=\"why is a cursor called a caret\">"),
        "{caret}"
    );
    // Hyphens, and only hyphens: no underscore rule, no case rule.
    let under = out.get("/an_underscored_name/").expect("the row rendered");
    assert!(
        under.contains("<title>an_underscored_name</title>"),
        "{under}"
    );
    let caps = out.get("/ALL-CAPS-HERE/").expect("the row rendered");
    assert!(caps.contains("<title>ALL CAPS HERE</title>"), "{caps}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// The rung is the BOTTOM one: front matter beats it, and so does anything
/// that arrives before it. A degenerate row's implied title is what a row gets
/// when nothing else offered one — not a value that overwrites what did.
///
/// Mutation: swap the `match` arms in either loader's title so the implied
/// title wins, and the titled page loses its own name.
#[test]
fn front_matter_beats_the_implied_title() {
    let dir = site(
        "rung",
        &format!(
            "{HEAD}\n  [[collections.rules]]\n  match = \"**/*.md\"\n  \
             route = \"/{{stem}}/\"\n  defaults = {{ shell = \"html\" }}\n"
        ),
        &[
            ("named-page.md", "---\ntitle: A Proper Name\n---\nBody.\n"),
            ("unnamed-page.md", "Body.\n"),
        ],
    );
    let (out, warnings) = build(&dir);

    let named = out.get("/named-page/").expect("the row rendered");
    assert!(named.contains("<title>A Proper Name</title>"), "{named}");
    let unnamed = out.get("/unnamed-page/").expect("the row rendered");
    assert!(unnamed.contains("<title>unnamed page</title>"), "{unnamed}");
    // Only the identity-less one is degenerate; the titled page has a block.
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("unnamed-page.md"), "{warnings:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

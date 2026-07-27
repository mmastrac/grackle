//! One shell axis, answering on a whole site (IO.md §4, item I2).
//!
//! A site rendered once, for the same reason `io_facts.rs` is a site: what
//! this asserts is a **set membership** — which routes a fold selected when it
//! filtered on `shell` — and every mechanism between the declaration and the
//! answer is a different file. The base's rule defaults write the field, the
//! cascade types it, the row route copies it, the view route mints it, and the
//! filter reads it; a unit test on any one of those passes against an engine
//! where the next one never happened.
//!
//! I1 measured the two gaps this closes, on a site exactly like this one:
//! `shell == "html"` selected **0** rows (absent is Null and Null matches
//! nothing) and `shell == "atom"` selected **0** routes (a view's
//! serialization was a declaration, not a field). Both probes are below.

use std::path::{Path, PathBuf};

/// One corpus with every shape the axis has an opinion about, under the BASE
/// config — which is the subject as much as the rows are, since the defaults
/// that make `shell` answerable ship in `base.toml` rather than in Rust.
///
/// - a post, and a post with no block (the caret draft's shape) — the posts
///   rule declares `html` for both;
/// - a front-mattered page, and a front-mattered `index.md` — the second is
///   the interesting one: the index rule wins its ROUTE and declares no shell,
///   so its `html` comes from the front-matter rule matching alongside;
/// - a static `.html`, a static `index.html` and a `.txt` — the catch-all
///   declares `raw`, and the static index proves the index rule was right to
///   stay silent (half the files it routes are byte copies);
/// - the base's own feed and blog listing, which answer `atom` and `html`
///   without either being written anywhere.
fn site(who: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("grackle-io-shell-{who}"));
    let _ = std::fs::remove_dir_all(&dir);
    let files = [
        (
            "grackle.toml",
            "[site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\n\
             [routes.html_probe]\npath = \"/html.xml\"\n\
             shell = \"sitemap\"\nwhere = 'shell == \"html\"'\n\n\
             [routes.raw_probe]\npath = \"/raw.xml\"\n\
             shell = \"sitemap\"\nwhere = 'shell == \"raw\"'\n\n\
             [routes.atom_probe]\npath = \"/atom-probe.xml\"\n\
             shell = \"sitemap\"\nwhere = 'shell == \"atom\"'\n\n\
             [routes.null_probe]\npath = \"/null.xml\"\n\
             shell = \"sitemap\"\nwhere = '!shell'\n",
        ),
        (
            "_posts/2020-01-01-hello.md",
            "---\ntitle: Hello\n---\n\nProse.\n",
        ),
        ("_posts/2020-01-02-bare.md", "Prose, and no block at all.\n"),
        ("about.md", "---\ntitle: About\n---\n\nProse.\n"),
        ("guide/index.md", "---\ntitle: Guide\n---\n\nProse.\n"),
        ("legacy.html", "<p>Bytes, verbatim.</p>\n"),
        ("frozen/index.html", "<p>An index nobody parses.</p>\n"),
        ("notes.txt", "Bytes, verbatim.\n"),
    ];
    for (rel, body) in files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().expect("a file has a directory")).unwrap();
        std::fs::write(&p, body).unwrap();
    }
    dir
}

/// Build, and hand back one probe's selected URL set.
fn urls(dir: &Path, route: &str) -> Vec<String> {
    let cfg = grackle::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    let mut db = grackle_source::load(&cfg).expect("the site loads");
    let (out, _) = grackle::build::render_site(&cfg, &mut db).expect("the site renders");
    let _ = std::fs::remove_dir_all(dir.join("_cache"));
    let xml = String::from_utf8(
        out.get(route)
            .unwrap_or_else(|| panic!("no route at {route} — routes: {:?}", out.keys()))
            .clone(),
    )
    .expect("a sitemap is utf-8");
    xml.lines()
        .filter_map(|l| l.strip_prefix("<loc>")?.strip_suffix("</loc>"))
        .map(|u| u.trim_start_matches("https://example.com").to_string())
        .collect()
}

/// `shell == "html"` selects the documents AND the listings, and nothing else.
///
/// The listings are the half I1 could not reach at all: `/`, `/blog/` and the
/// probe routes are view routes with no source file, and before I2 they
/// answered Null to every shell question. A view that declares no fold IS the
/// HTML shell, so it says so.
///
/// Mutations, each restored:
///
/// - delete `defaults = { shell = "html" }` from the base's front-matter rule
///   and `/about/` and `/guide/` leave the set — loudly, since they are still
///   *rendered* as HTML documents (the `_` arm in `build.rs` catches them),
///   which is the point: the fact goes quiet while the bytes do not move. They
///   land in `!shell` instead, which the fourth probe pins.
/// - delete it from the posts rule and both posts leave.
/// - delete the `shell` line from `view_fields` and `/` and `/blog/` leave.
///
/// The probe routes are absent from their own answer, and that is the column
/// being honest rather than a filter someone wrote: each probe is a
/// `shell = "sitemap"` fold, so it leaves through the sitemap shell and says
/// so. Before I2 nothing about a route could tell you that.
#[test]
fn the_html_shell_selects_documents_and_listings() {
    let dir = site("html");
    assert_eq!(
        urls(&dir, "/html.xml"),
        [
            "/",
            "/about/",
            "/blog/",
            "/blog/2020/01/01/hello/",
            "/blog/2020/01/02/bare/",
            "/guide/",
        ],
        "every document the engine themes, and every listing"
    );
}

/// `raw` is the transparent shell, and the byte copies are what wear it — the
/// static `index.html` included, which is why the base's index rule declares
/// no shell of its own. It routes rendered pages and byte copies alike, and
/// the front-matter gate on the two rules beside it is what tells them apart.
///
/// Mutation: delete `defaults = { shell = "raw" }` from the base's catch-all
/// and this set empties.
#[test]
fn the_raw_shell_is_the_byte_copies_static_indexes_included() {
    let dir = site("raw");
    assert_eq!(
        urls(&dir, "/raw.xml"),
        ["/frozen/", "/legacy.html", "/notes.txt"],
        "bytes at their literal paths, and an index served as its directory"
    );
}

/// The second gap I1 measured, closed: a view route answers the serialization
/// it leaves through, so the feed is findable by what it IS rather than by the
/// name someone gave the route.
///
/// Mutation: drop the `unwrap_or(VIEW_DEFAULT)` and mint Null for an
/// undeclared view instead — this still passes, and
/// `the_html_shell_selects_documents_and_listings` is the one that fails. Both
/// halves of the column are load-bearing.
#[test]
fn a_feed_route_answers_atom() {
    let dir = site("atom");
    assert_eq!(urls(&dir, "/atom-probe.xml"), ["/atom.xml"]);
}

/// What is left over, stated rather than left to be discovered — this is the
/// item's honest remainder and the reason the sitemap's `dir || ext == "html"`
/// did NOT migrate to `shell == "html"` here (IO.md §11).
///
/// An objects-collection row never takes a rule default at all: the loader
/// builds it from `Default::default()`, so no cascade runs over it and no
/// `defaults = { shell = … }` on the objects rule would be read. Every image
/// on a site therefore answers Null, and a filter written as `shell == "raw"`
/// would silently drop them where `dir || ext == "html"` never listed them in
/// the first place.
///
/// Mutation: add `defaults = { shell = "raw" }` to the base's objects rule and
/// this test does not move, which is the proof that the gap is in the loader
/// rather than in the config.
#[test]
fn an_object_row_answers_no_shell_at_all() {
    let dir = site("null");
    std::fs::write(dir.join("logo.png"), b"not really a png").unwrap();
    let null = urls(&dir, "/null.xml");
    assert!(
        null.contains(&"/logo.png".to_string()),
        "an object takes no rule defaults, so it wears no shell: {null:?}"
    );
    // An all-outputs fold answers the column too — its route carried no
    // fields AT ALL before I2, so without its `view_fields` call the four
    // probes would be sitting in this set.
    for probe in ["/html.xml", "/raw.xml", "/atom-probe.xml", "/null.xml"] {
        assert!(
            !null.contains(&probe.to_string()),
            "a fold's route leaves through the sitemap shell: {null:?}"
        );
    }
}

/// A per-member route is one output, and the member decides which shell it
/// left through (q53's md twin: one row, two serializations, two URLs). The
/// column has to agree with the bytes, or a fold over the route pool would
/// describe the canonical form twice.
///
/// `light_html` is declared FIRST so it is the canonical member, because an
/// all-outputs fold sees canonical members only (q53: an alternate is not a
/// second document). The row itself takes `html` from the base's front-matter rule,
/// so the two disagree and the probe can tell which one answered.
///
/// Mutation: delete the `m.field == "shell"` correction in `load.rs`'s route
/// constructor and the member answers the ROW's `html` — it joins the html set
/// while rendering the light tier, which is the column lying about the file it
/// names.
#[test]
fn an_axis_member_answers_its_own_shell() {
    let dir = std::env::temp_dir().join("grackle-io-shell-axis");
    let _ = std::fs::remove_dir_all(&dir);
    let files = [
        (
            "grackle.toml",
            "[site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\n\
             [axes.serialization]\nvalues = [\"light_html\", \"html\"]\nfield = \"shell\"\n\n\
             [[collections]]\nkind = \"tree\"\nsource = \".\"\n\n\
             [[collections.rules]]\nmatch = \"tiers.md\"\nfront_matter = true\n\
             route = \"/tiers/{serialization}/\"\n\n\
             [routes.html_probe]\npath = \"/html.xml\"\n\
             shell = \"sitemap\"\nwhere = 'shell == \"html\"'\n\n\
             [routes.light_probe]\npath = \"/light.xml\"\n\
             shell = \"sitemap\"\nwhere = 'shell == \"light_html\"'\n",
        ),
        ("tiers.md", "---\ntitle: Tiers\n---\n\nProse.\n"),
    ];
    for (rel, body) in files {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(rel), body).unwrap();
    }
    assert_eq!(urls(&dir, "/light.xml"), ["/tiers/light_html/"]);
    assert_eq!(
        urls(&dir, "/html.xml"),
        Vec::<String>::new(),
        "the row wears `html` and the member does not — the member answers"
    );
}

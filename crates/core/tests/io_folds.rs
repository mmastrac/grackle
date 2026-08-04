//! A fold with no `from` reads every output (IO.md §4, item I3).
//!
//! This is a **respelling**, not a new power: `from = "*"` read the finished
//! route set, and so does an absent `from` — the same pool, said by leaving a
//! line out instead of by naming a wildcard. What the item buys is that the
//! spelling now follows from the model: a fold sits on a query over outputs,
//! so "all of them" is a query it can serialize, while a map shell wraps ONE
//! output and a view wearing one is a listing that has to say what it lists.
//!
//! A site rather than a unit test, for `io_shell.rs`'s reason: what these
//! assert is a **set membership**, and the mechanisms between a missing config
//! line and a `<loc>` are five files apart (`Config::check` admits it,
//! `build_views` skips it, `build_pool_folds` mints its route,
//! `force_route_fields` fills the pool, `resolve_pool_folds` filters it).
//!
//! The rung-0 side of this item needed no code and has no test here: MERGE.md
//! R6 already moved `force_route_fields` above `resolve_pool_folds`, so both
//! pools see forced fields, and `profile_force.rs` guards it in both
//! directions.

use std::path::PathBuf;

mod support;
use support::render;

/// One corpus with a document, a byte copy, and the base's own feed.
///
/// `all` is the item's subject — a fold shell, no `from`, no `where` — so its
/// `<loc>` list is literally every route this site publishes. `documents` is
/// the same shape with a predicate, because "reads all outputs" has to mean
/// the pool it *starts* from rather than the set it ends with.
fn site(who: &str) -> PathBuf {
    let files = [
        (
            "grackle.toml",
            "[site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\n\
             [routes.all]\npath = \"/all.xml\"\nshell = \"sitemap\"\n\n\
             [routes.documents]\npath = \"/documents.xml\"\n\
             shell = \"sitemap\"\nwhere = 'shell == \"html\"'\n",
        ),
        (
            "_posts/2020-01-01-hello.md",
            "---\ntitle: Hello\n---\n\nProse.\n",
        ),
        ("notes.txt", "Bytes, verbatim.\n"),
    ];
    support::site("io-folds", who, &files)
}

fn text(out: &grackle_core::build::SiteOutput, route: &str) -> String {
    String::from_utf8(
        out.get(route)
            .unwrap_or_else(|| panic!("no route at {route} — routes: {:?}", out.keys()))
            .clone(),
    )
    .expect("the artifact is utf-8")
}

fn locs(out: &grackle_core::build::SiteOutput, route: &str) -> Vec<String> {
    support::sitemap_locs(&text(out, route))
}

/// **The respelling, landed.** A fold shell with no `from` lists every output
/// — the whole route set, its own artifact and its neighbour's included.
///
/// The unfiltered pool is what makes this a statement about `from` rather than
/// about `where`: `/all.xml` names the `.txt` byte copy, the feed, and both
/// fold artifacts, none of which a row query could ever reach. `/documents.xml`
/// is the same pool narrowed, so the two together say the pool is the start
/// and the predicate is the end.
///
/// Mutations, each restored:
///
/// - make `View::reads_all_outputs` return `false`: `build_pool_folds` mints
///   nothing and `/all.xml` does not exist at all — the panic in `text` names
///   the routes that do;
/// - make it return `true` unconditionally: the base's own `[routes.blog_index]`
///   becomes a listing with no pool and the load fails on it, which is the
///   guard below firing on the engine instead of on a site.
#[test]
fn a_fold_with_no_from_reads_every_output() {
    let out = render(&site("all"));
    let all = locs(&out, "/all.xml");
    for url in [
        "/",
        "/blog/",
        "/blog/2020/01/01/hello/",
        "/notes.txt",
        "/atom.xml",
        "/all.xml",
        "/documents.xml",
        "/sitemap.xml",
    ] {
        assert!(all.contains(&url.to_string()), "{url} missing from {all:?}");
    }
    // And the predicate narrows that pool rather than choosing another one:
    // the byte copy and the two XML artifacts leave, the documents stay.
    let docs = locs(&out, "/documents.xml");
    assert!(
        docs.contains(&"/blog/2020/01/01/hello/".to_string()),
        "{docs:?}"
    );
    assert!(docs.contains(&"/blog/".to_string()), "{docs:?}");
    for url in ["/notes.txt", "/atom.xml", "/all.xml"] {
        assert!(!docs.contains(&url.to_string()), "{url} in {docs:?}");
    }
}

/// **The feed is not a fold over the pool, and it did not move.** `from`
/// naming a set selects that set's rows — `[routes.feed] from = "published"`
/// keeps meaning what it means (IO.md §4).
///
/// At today's fidelity a fold over a set consumes the set's ROWS directly,
/// which is "those inputs' outputs" said in the only vocabulary that exists
/// yet; the join makes the selection output-mediated at **I9**. What this
/// pins is the half that must not change either way: the feed's entries are
/// the query's members, so the route pool's sourceless artifacts — the
/// sitemap, the feed itself — can never appear in it.
#[test]
fn a_fold_over_a_set_still_reads_that_set() {
    let out = render(&site("feed"));
    let feed = text(&out, "/atom.xml");
    assert!(feed.contains("<![CDATA[Hello]]>"), "{feed}");
    assert_eq!(feed.matches("<entry>").count(), 1, "{feed}");
    // The route pool's sourceless artifacts are exactly what a row query
    // cannot reach, so their absence is the shape's signature.
    for absent in ["/all.xml", "/documents.xml", "/sitemap.xml", "/notes.txt"] {
        assert!(!feed.contains(absent), "{absent} in the feed:\n{feed}");
    }
}

/// **A script shell has to say what it eats** (IO.md §4, IR1(a)).
///
/// I3 let every registered `[shells.*]` name be `from`-less on the grounds
/// that a script shell is a fold by arity. Arity was the right reading and the
/// wrong conclusion, and batch review I-A proved it live: the engine's folds
/// read the route pool *themselves* (`resolve_pool_folds` fills
/// `route_members`), while `build.rs`'s script pass reads `r.members` — the
/// ROW projection, which a pool fold never fills. So a `from`-less script
/// shell loaded clean and published `"rows":[]` at the URL its author asked a
/// query for: the silent-empty disease, one family over.
///
/// Mutation: delete the `registered` arm in `shell::check_absent_from` and
/// this site LOADS — `/probe.json` is published, with an empty `rows`. That is
/// the assertion below the error, and it is why the guard is a load error
/// rather than a note in a doc comment.
#[test]
fn a_script_shell_with_no_from_is_a_load_error() {
    let dir = site("script-orphan");
    let cfg = dir.join("grackle.toml");
    let base = std::fs::read_to_string(&cfg).expect("the fixture wrote it");
    std::fs::write(
        &cfg,
        format!(
            "{base}\n[shells.echo]\ncommand = \"cat\"\n\n\
             [routes.probe]\npath = \"/probe.json\"\nshell = \"echo\"\n"
        ),
    )
    .unwrap();
    let e = format!(
        "{:#}",
        grackle_core::config::Config::load(&cfg)
            .expect_err("a script shell must be told what to eat")
    );
    assert!(e.contains("view probe: shell = \"echo\""), "{e}");
    assert!(e.contains("is a script shell and has no `from`"), "{e}");
    // It says WHY — the payload's rows — and names the shells that really do
    // read the pool, so the reader can tell "I forgot a line" from "I wanted
    // the sitemap's power".
    assert!(e.contains("the payload's `rows`"), "{e}");
    assert!(e.contains("atom, sitemap, search"), "{e}");
}

/// **The control, both halves.** The narrowing is about the missing `from`,
/// not about script shells: one WITH a `from` still eats its rows, and the
/// engine's own folds are still legal without one.
///
/// `examples/field-notes` is the live user (`[shells.llms]` +
/// `[routes.llms] from = "published"`), and it must keep building; this is
/// the same shape in a test the item can mutate. `cat` is the identity
/// shell, so the payload the engine wrote is the artifact it published and
/// the assertion can read it directly.
#[test]
fn a_script_shell_with_a_from_still_eats_its_rows() {
    let dir = site("script-fed");
    let cfg = dir.join("grackle.toml");
    let base = std::fs::read_to_string(&cfg).expect("the fixture wrote it");
    std::fs::write(
        &cfg,
        format!(
            "{base}\n[shells.echo]\ncommand = \"cat\"\n\n\
             [routes.probe]\npath = \"/probe.json\"\nfrom = \"posts\"\n\
             shell = \"echo\"\n"
        ),
    )
    .unwrap();
    let out = render(&dir);
    let payload = text(&out, "/probe.json");
    assert!(payload.contains("\"shell\":\"echo\""), "{payload}");
    assert!(payload.contains("\"title\":\"Hello\""), "{payload}");
    assert!(!payload.contains("\"rows\":[]"), "{payload}");
    // And the engine's folds are untouched by the narrowing: `all` is still
    // `from`-less, and still lists every output including this one.
    assert!(locs(&out, "/all.xml").contains(&"/probe.json".to_string()));
}

/// **The control: absent `from` is legal only under a fold.** Every other view
/// is a listing, and a listing has to say what it lists.
///
/// This is the requirement that existed before the item (serde's `missing
/// field \`from\``) kept where it was, in the one place it stops being right.
/// Without the check the entry does not fail — it succeeds *as a fold*,
/// because `reads_all_outputs` is one field: `build_views` skips it,
/// `build_pool_folds` mints its route, and the author gets an empty listing
/// where they asked for a query. Silence, not a crash, which is why the guard
/// is a load error rather than a comment.
///
/// Mutation: delete the `check_absent_from` call in `Config::check` and the
/// site LOADS, publishing `/orphan/` with no members at all.
#[test]
fn absent_from_without_a_fold_is_a_load_error() {
    let dir = site("orphan");
    let cfg = dir.join("grackle.toml");
    let base = std::fs::read_to_string(&cfg).expect("the fixture wrote it");
    std::fs::write(
        &cfg,
        format!("{base}\n[routes.orphan]\npath = \"/orphan/\"\nlayout = \"card\"\n"),
    )
    .unwrap();
    let e = format!(
        "{:#}",
        grackle_core::config::Config::load(&cfg).expect_err("a listing must say what it lists")
    );
    assert!(e.contains("view orphan: no `from`"), "{e}");
    assert!(e.contains("Only a FOLD shell reads every output"), "{e}");
    // It names what this view leaves through, so the reader can tell an
    // arity mistake from a forgotten line, and lists the way out of both.
    assert!(e.contains("the HTML listing"), "{e}");
    assert!(e.contains("atom, sitemap, search"), "{e}");
}

/// **The hard cutoff.** `from = "*"` is a value, not a key, so
/// `deny_unknown_fields` never sees it: post-cutoff it is simply a name that
/// names nothing, and it lands in `check_base` with every other typo.
///
/// The generic arm's sentence — "`from = "*"` is neither a collection, a set
/// nor a route (collections: …; sets and routes: …)" — is true and useless:
/// it sends its reader to look for a collection called `*`, when the fix is
/// to delete a line. So the literal gets one sentence of its own. It is a
/// REAL error, not a teaching error (MERGE.md §4): the value is invalid now,
/// and the message says what a fold does instead rather than how to migrate.
///
/// Mutation: delete the `From::One("*")` arm in `Config::check_base` and the
/// load still fails — by the generic sentence, which is the diagnosis this
/// test exists to keep.
#[test]
fn the_star_spelling_is_gone() {
    let dir = site("star");
    let cfg = dir.join("grackle.toml");
    let base = std::fs::read_to_string(&cfg).expect("the fixture wrote it");
    std::fs::write(
        &cfg,
        base.replace(
            "[routes.all]\npath = \"/all.xml\"\n",
            "[routes.all]\npath = \"/all.xml\"\nfrom = \"*\"\n",
        ),
    )
    .unwrap();
    let cfg =
        grackle_core::config::Config::load(&cfg).expect("the config still parses — `*` is a VALUE");
    let e = format!(
        "{:#}",
        grackle_source::load(&cfg).expect_err("and it names nothing")
    );
    assert!(e.contains("all: `from = \"*\"` names nothing"), "{e}");
    assert!(e.contains("having no `from` at all"), "{e}");
    assert!(
        !e.contains("is neither a collection"),
        "the literal gets its own sentence, not the generic one: {e}"
    );
}

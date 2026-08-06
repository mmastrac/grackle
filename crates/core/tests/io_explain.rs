//! What `grackle explain` says a row IS.
//!
//! The line under test used to be `println!("kind        post")` — a literal,
//! printed for every row, so a `.txt` copied verbatim reported itself a post.
//! A literal passes any test written against one row shape, so this asserts
//! the block for **two rows that must disagree in every fact**: a post with a
//! front-matter block, and a byte copy with none. The block also
//! carries a fourth, `rule` — the glob that CLAIMED the row, which is the
//! ordering law's one observable and the only way to ask why a file landed in
//! the scope it did.
//!
//! The loader is in the loop deliberately. `row_facts` reads three fields, and
//! a unit test over a hand-built `Row` would prove only that `format!`
//! interpolates — it would pass against an engine that never gave a byte copy
//! the `raw` shell.
//!
//! The second test covers the block below it, `row_fields`: the cascade
//! keys are named fields on `Row` *and* declared columns in `Row.fields`, so
//! `explain` printed `layout` twice for every row that resolved one. The
//! loader earns its place there for the same reason — whether a value reaches
//! both places is a fact about the load, not about the printer.
//!
//! The third covers the `rendered` line — the one DERIVED value in the
//! block, and the only one a reader cannot read off the file. It needs four
//! rows rather than two, because the law it prints is a disjunction and each
//! clause has a shape that is the only witness against getting it wrong.
//!
//! `front_mattered` carries its PROVENANCE — `true (block)` here,
//! `true (sidecar)` on a row whose identity arrived from a second file — because
//! once the fact has two sources the law stops being re-derivable from the
//! printed pair. The sidecar half is pinned in `io_sidecar.rs`, beside the
//! mechanism it is about; what these two rows hold is the other half, which is
//! that identity written IN the file still says so.

use std::path::PathBuf;

mod support;

/// Four rows, chosen so that no two agree on the whole block.
///
/// The first two are a pair, and every fact differs across them: a dated
/// post the posts scope claims (identity in the file, the html shell) and a
/// `.txt` the tree rule copies verbatim (no identity, the raw shell).
///
/// The post carries all three of the cascade keys `row_fields` names, and one
/// declared field that is not a cascade key (`minutes`), so the skip is
/// pinned as "skip these four names" rather than "skip the dump".
///
/// The other two are one per clause of the rendering law, and both are
/// corpus shapes rather than invented ones:
///
/// - **The degenerate row.** A blockless `.md` under `_posts/`, which the base's
///   posts rule claims with `defaults = { shell = "html" }` and no front-matter
///   gate: `front_mattered false`, `rendered true`. This is grack.com's
///   `_drafts/caret/…`, reproduced under the base config.
/// - **The pane.** A front-mattered `.html` whose front matter says
///   `shell: raw` — `examples/field-notes`' `demos/pane.html`, down to the
///   path. It renders (identity), and the `raw` shell then emits the result
///   verbatim: `shell raw`, `rendered true`.
///
/// `whose` keeps the tests off each other's tree: they run in parallel and
/// each deletes its directory at both ends.
fn site(whose: &str) -> PathBuf {
    let files = [
        (
            "grackle.toml",
            "[site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\
             \n[schema]\nminutes = { type = \"int\" }\n",
        ),
        (
            "_posts/2020-01-01-hello.md",
            "---\ntitle: Hello\ntheme: ledger\ntoc: true\nminutes: 4\n---\n\nProse.\n",
        ),
        ("notes.txt", "Bytes, verbatim.\n"),
        (
            "_posts/2020-02-02-why-is-a-cursor-called-a-caret.md",
            "No block at all — this file is all body.\n",
        ),
        (
            "demos/pane.html",
            "---\ntitle: Glass pane\nshell: raw\n---\n<div class=\"pane\"></div>\n",
        ),
        // The fifth: an image the base's objects rule DECLINES to
        // route (`embed = true`), so its address is the policy's. Written as
        // text rather than as PNG bytes because nothing here decodes it — the
        // block under test is about addresses.
        ("assets/kite.png", "not a real png, and nothing reads it\n"),
    ];
    support::site("io-explain", whose, &files)
}

/// Mutation: put any one of the four values back as a literal — `"posts"`,
/// `"**/*.{md,markdown}"`, `"html"`, `true` — and the byte copy's assertion
/// fails on that line. (The original lie, `kind post` for everything, is the
/// `collection` case.)
#[test]
fn explain_reads_the_row_rather_than_a_literal() {
    let dir = site("facts");
    let db = support::load(&dir);
    let facts = |url: &str| {
        let r = db
            .by_url
            .get(url)
            .and_then(|k| db.rows.get(k))
            .unwrap_or_else(|| panic!("no row at {url}"));
        grackle_core::debug::row_facts(r)
    };

    assert_eq!(
        facts("/blog/2020/01/01/hello/"),
        "collection  posts\nrule        **/*.{md,markdown}\nshell       html\n\
         front_mattered true (block)\nrendered    true\n\
         output      /blog/2020/01/01/hello/\n",
        "a post: the scope that claimed it, the RULE of that scope that did the \
         claiming, the shell it leaves through, identity, and where it LANDS"
    );
    assert_eq!(
        facts("/notes.txt"),
        "collection  entries\nrule        **/*\nshell       raw\nfront_mattered false\n\
         rendered    false\noutput      /notes.txt\n",
        "a byte copy disagrees with the post in all four: a different scope, a \
         different rule of it, a different shell, no identity — and it used to \
         report itself a post in the one fact printed. It agrees in the fifth: \
         it lands, which is why `output` is not a spelling of `rendered`"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// **The two address slots, as `explain` prints them**.
///
/// An embed-addressed row is the one shape where the block's `output` dash has
/// to be read together with a line ABOVE it: `url` is empty, `output` is a
/// dash, and without the reason those two together say "this row is broken"
/// rather than "an `<img>` can reach this row and a link cannot". So the dash
/// carries its fourth reason and `strong_url` is printed beside it — and only
/// there, because a `strong_url -` on every routed row of every site would
/// report the policy off rather than the row routed.
///
/// Found by `rel` rather than by URL, which is itself the claim: there is no
/// URL to look it up by.
///
/// Mutations, each red: give the fourth arm the bare dash (the reason goes,
/// and the row is indistinguishable from one no rule mentioned); print
/// `strong_url` unconditionally (the post's block grows a line that says
/// nothing); print the slot and drop the reason (the pair stops being
/// readable).
#[test]
fn explain_prints_the_second_address_slot_and_the_reason_for_the_dash() {
    let dir = site("strong");
    let db = support::load(&dir);
    let kite = db
        .rows
        .iter()
        .find(|r| r.rel.to_string_lossy() == "assets/kite.png")
        .expect("the image is a row, addressed or not");
    let block = grackle_core::debug::row_facts(kite);
    assert!(
        block.contains("output      - (embed-addressed — nothing has embedded it)\n"),
        "the fourth reason a dash can have: {block}"
    );
    assert!(
        block.contains("strong_url  /static/"),
        "and the slot that says what CAN reach it: {block}"
    );
    // The control, in the same site: a routed row prints no slot at all.
    let post = db
        .by_url
        .get("/blog/2020/01/01/hello/")
        .and_then(|k| db.rows.get(k))
        .expect("the post is a row");
    assert!(
        !grackle_core::debug::row_facts(post).contains("strong_url"),
        "a routed row has one address and says so by silence"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The block's last line is the rendering law's answer —
/// `front_mattered || shell ∈ {html, light_html}` — printed beneath the two
/// facts it reads, because the derivation is what a reader of `explain` cannot
/// do in their head from a row they are already confused about.
///
/// Four rows and no two alike. The first two (`true`/`true` and
/// `false`/`false`) agree with both halves of the law and so witness nothing on
/// their own; the two below are each the sole witness against one wrong
/// implementation:
///
/// - the **degenerate** row reads `front_mattered false / rendered true` — the
///   pair that teaches the law, and the row that fails an engine printing
///   `r.front_mattered` under a `rendered` label (the tree loader's old
///   answer);
/// - the **pane** reads `shell raw / rendered true` — the row that fails "the
///   shell decides", which is the law that byte-copies a front-mattered file
///   and ships its `---` block.
///
/// Mutations, each red: hardcode `true` and the byte copy fails; hardcode
/// `false` and the other three do; print `r.front_mattered` and the degenerate
/// row fails alone; print `is_document(shell)` and the pane fails alone. The
/// loader is in the loop for the reason the file's other tests keep it there —
/// `rendered` is *stored* on the row, decided at load by `shell::renders`, so a
/// hand-built `Row` would prove only that `format!` interpolates a bool.
#[test]
fn explain_prints_the_rendering_law_beside_the_facts_it_reads() {
    let dir = site("rendered");
    let db = support::load(&dir);
    let facts = |url: &str| {
        let r = db
            .by_url
            .get(url)
            .and_then(|k| db.rows.get(k))
            .unwrap_or_else(|| panic!("no row at {url}"));
        grackle_core::debug::row_facts(r)
    };

    assert_eq!(
        facts("/blog/2020/02/02/why-is-a-cursor-called-a-caret/"),
        "collection  posts\nrule        **/*.{md,markdown}\nshell       html\n\
         front_mattered false\nrendered    true\n\
         output      /blog/2020/02/02/why-is-a-cursor-called-a-caret/\n",
        "the degenerate row: no identity, a document shell, and it renders — \
         the law's second clause, and the pair a reader learns it from. It \
         lands like any other row: identity is not what mints a URL"
    );
    assert_eq!(
        facts("/demos/pane/"),
        "collection  entries\nrule        **/*.{html,md}\nshell       raw\n\
         front_mattered true (block)\nrendered    true\noutput      /demos/pane/\n",
        "the pane: identity plus the transparent shell renders, and `raw` then \
         emits the result verbatim — the law's first clause, and the row a \
         shell-only law would ship the `---` block for"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// A cascade key is a named field on `Row` AND a declared column in
/// `Row.fields`, so `explain` printed cascade keys twice for every row that
/// had one. Each of `slot`/`theme` lives on exactly one line here; ordinary
/// declared fields (`toc`, `minutes`, …) appear only in the dump when set.
///
/// Mutations, each red: drop the `CASCADE` skip in `row_fields` and the post
/// grows a second `slot`/`theme` from the dump; drop any named line and the
/// `.txt` — which resolves none — loses its answer entirely, which is the
/// failure the dump alone cannot see.
#[test]
fn explain_prints_each_cascade_key_exactly_once() {
    let dir = site("cascade");
    let db = support::load(&dir);
    let fields = |url: &str| {
        let r = db
            .by_url
            .get(url)
            .and_then(|k| db.rows.get(k))
            .unwrap_or_else(|| panic!("no row at {url}"));
        grackle_core::debug::row_fields(r)
    };

    assert_eq!(
        fields("/blog/2020/01/01/hello/"),
        "slot        -\ntheme       ledger\ndate        2020-01-01\nlocale      en\nminutes     4\ntoc         true\n",
        "a row that resolved theme (slot absent): one named line each, and the \
         dump still carries ordinary fields including toc and file-pattern date; \
         base's monolingual pairing axis stamps locale"
    );
    assert_eq!(
        fields("/notes.txt"),
        "slot        -\ntheme       -\nlocale      en\n",
        "a row that resolved no cascade keys: base still stamps the pairing \
         axis, so locale appears beside the named cascade dashes"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

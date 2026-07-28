//! What `grackle explain` says a row IS (IO.md §3, IR2 and IR3).
//!
//! The line under test used to be `println!("kind        post")` — a literal,
//! printed for every row, so a `.txt` copied verbatim reported itself a post.
//! A literal passes any test written against one row shape, so this asserts
//! the block for **two rows that must disagree in every fact**: a post with a
//! front-matter block, and a byte copy with none. Since IO.md I7d the block
//! carries a fourth, `rule` — the glob that CLAIMED the row, which is the
//! ordering law's one observable and the only way to ask why a file landed in
//! the scope it did.
//!
//! The loader is in the loop deliberately. `row_facts` reads three fields, and
//! a unit test over a hand-built `Row` would prove only that `format!`
//! interpolates — it would pass against an engine that never gave a byte copy
//! the `raw` shell.
//!
//! The second test covers the block below it, `row_fields` (IR3): the cascade
//! keys are named fields on `Row` *and* declared columns in `Row.fields`, so
//! `explain` printed `layout` twice for every row that resolved one. The
//! loader earns its place there for the same reason — whether a value reaches
//! both places is a fact about the load, not about the printer.

use std::path::PathBuf;

/// Two rows, chosen because every fact differs across them: a dated post the
/// posts scope claims (identity in the file, the html shell) and a `.txt` the
/// tree rule copies verbatim (no identity, the raw shell).
///
/// The post carries all three of the cascade keys `row_fields` names, and one
/// declared field that is not a cascade key (`minutes`), so IR3's skip is
/// pinned as "skip these four names" rather than "skip the dump".
///
/// `whose` keeps the two tests off each other's tree: they run in parallel and
/// each deletes its directory at both ends.
fn site(whose: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("grackle-io-explain-{whose}"));
    let _ = std::fs::remove_dir_all(&dir);
    let files = [
        (
            "grackle.toml",
            "[site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\
             \n[schema]\nminutes = { type = \"int\" }\n",
        ),
        (
            "_posts/2020-01-01-hello.md",
            "---\ntitle: Hello\nlayout: post\ntheme: ledger\ntoc: true\nminutes: 4\n---\n\nProse.\n",
        ),
        ("notes.txt", "Bytes, verbatim.\n"),
    ];
    for (rel, body) in files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().expect("a file has a directory")).unwrap();
        std::fs::write(&p, body).unwrap();
    }
    dir
}

/// Mutation: put any one of the four values back as a literal — `"posts"`,
/// `"**/*.{md,markdown}"`, `"html"`, `true` — and the byte copy's assertion
/// fails on that line. (The original lie, `kind post` for everything, is the
/// `collection` case.)
#[test]
fn explain_reads_the_row_rather_than_a_literal() {
    let dir = site("facts");
    let cfg = grackle::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    let db = grackle_source::load(&cfg).expect("the site loads");
    let facts = |url: &str| {
        let r = db
            .by_url
            .get(url)
            .and_then(|k| db.rows.get(k))
            .unwrap_or_else(|| panic!("no row at {url}"));
        grackle::debug::row_facts(r)
    };

    assert_eq!(
        facts("/blog/2020/01/01/hello/"),
        "collection  posts\nrule        **/*.{md,markdown}\nshell       html\n\
         front_mattered true\n",
        "a post: the scope that claimed it, the RULE of that scope that did the \
         claiming, the shell it leaves through, identity"
    );
    assert_eq!(
        facts("/notes.txt"),
        "collection  entries\nrule        **/*\nshell       raw\nfront_mattered false\n",
        "a byte copy disagrees with the post in all four: a different scope, a \
         different rule of it, a different shell, no identity — and it used to \
         report itself a post in the one fact printed"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// IO.md IR3: a cascade key is a named field on `Row` AND a declared column in
/// `Row.fields`, so `explain` printed `layout` twice for every row that had
/// one. Each of the three lives on exactly one line here.
///
/// Mutations, each red: drop the `CASCADE` skip in `row_fields` and the post
/// grows a second `layout`/`theme`/`toc` from the dump; drop any named line
/// and the `.txt` — which resolves none of the three — loses its answer
/// entirely, which is the failure the dump alone cannot see.
#[test]
fn explain_prints_each_cascade_key_exactly_once() {
    let dir = site("cascade");
    let cfg = grackle::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    let db = grackle_source::load(&cfg).expect("the site loads");
    let fields = |url: &str| {
        let r = db
            .by_url
            .get(url)
            .and_then(|k| db.rows.get(k))
            .unwrap_or_else(|| panic!("no row at {url}"));
        grackle::debug::row_fields(r)
    };

    assert_eq!(
        fields("/blog/2020/01/01/hello/"),
        "layout      post\ntheme       ledger\ntoc         true\nminutes     4\n",
        "a row that resolved all three: one line each, and the dump still \
         carries the field that is not a cascade key"
    );
    assert_eq!(
        fields("/notes.txt"),
        "layout      -\ntheme       -\ntoc         false\n",
        "a row that resolved none: the dump would have printed nothing at all, \
         so the named lines are the only answer"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

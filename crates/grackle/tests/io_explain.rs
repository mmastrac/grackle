//! What `grackle explain` says a row IS (IO.md §3, IR2).
//!
//! The line under test used to be `println!("kind        post")` — a literal,
//! printed for every row, so a `.txt` copied verbatim reported itself a post.
//! A literal passes any test written against one row shape, so this asserts
//! the block for **two rows that must disagree in all three facts**: a post
//! with a front-matter block, and a byte copy with none.
//!
//! The loader is in the loop deliberately. `row_facts` reads three fields, and
//! a unit test over a hand-built `Row` would prove only that `format!`
//! interpolates — it would pass against an engine that never gave a byte copy
//! the `raw` shell.

use std::path::PathBuf;

/// Two rows, chosen because every fact differs across them: a dated post the
/// posts scope claims (identity in the file, the html shell) and a `.txt` the
/// tree rule copies verbatim (no identity, the raw shell).
fn site() -> PathBuf {
    let dir = std::env::temp_dir().join("grackle-io-explain");
    let _ = std::fs::remove_dir_all(&dir);
    let files = [
        (
            "grackle.toml",
            "[site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n",
        ),
        (
            "_posts/2020-01-01-hello.md",
            "---\ntitle: Hello\n---\n\nProse.\n",
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

/// Mutation: put any one of the three values back as a literal — `"posts"`,
/// `"html"`, `true` — and the byte copy's assertion fails on that line. (The
/// original lie, `kind post` for everything, is the `collection` case.)
#[test]
fn explain_reads_the_row_rather_than_a_literal() {
    let dir = site();
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
        "collection  posts\nshell       html\nfront_mattered true\n",
        "a post: the scope that claimed it, the shell it leaves through, identity"
    );
    assert_eq!(
        facts("/notes.txt"),
        "collection  entries\nshell       raw\nfront_mattered false\n",
        "a byte copy disagrees with the post in all three — and used to report \
         itself a post in the one fact printed"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

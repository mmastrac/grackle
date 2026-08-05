//! `front_mattered`, and what it does and does not agree with.
//!
//! A whole site rendered once, so it belongs with the fixtures by `fixtures.rs`'s
//! own line ("if the subject is *a site*, it belongs here"). It is not a fixture
//! because what it asserts is a *set membership* — which URLs an all-outputs
//! fold selected — and a fixture would express that as several kilobytes of blessed
//! HTML in which the one interesting line is a `<loc>`.
//!
//! The loader is the subject. `front_mattered` is a bit written in two places
//! (the posts loader from the raw file, the tree loader from the walk's peek)
//! and read in a third (the route pool), and a unit test on `Route::field`
//! would pass against a loader that never set it.

use std::path::PathBuf;

mod support;
use support::sitemap_urls as urls;

/// One corpus, holding every shape the fact has an opinion about:
///
/// - a post WITH a front-matter block — the ordinary document;
/// - a post WITHOUT one — parsed all the same, because the posts scope hands
///   it a date and a route, and grack.com really has one of these;
/// - a page with a block — a tree row the front-matter rule routed prettily;
/// - an `.html` with no block, and a `.txt` — bytes copied verbatim.
///
/// The probes are the sitemap's shape — a fold shell with no `from`, so it
/// reads every output — which is the shape the corpus filters this
/// item migrates are written in.
fn site(who: &str) -> PathBuf {
    let files = [
        (
            "grackle.toml",
            "[site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\n\
             [routes.identity]\npath = \"/identity.xml\"\n\
             shell = \"sitemap\"\nwhere = \"front_mattered\"\n\n\
             [routes.bytes]\npath = \"/bytes.xml\"\n\
             shell = \"sitemap\"\nwhere = \"!front_mattered\"\n\n\
             [routes.old_spelling]\npath = \"/old-spelling.xml\"\n\
             shell = \"sitemap\"\nwhere = 'kind == \"post\" || kind == \"page\"'\n",
        ),
        (
            "_posts/2020-01-01-hello.md",
            "---\ntitle: Hello\n---\n\nProse.\n",
        ),
        ("_posts/2020-01-02-bare.md", "Prose, and no block at all.\n"),
        ("about.md", "---\ntitle: About\n---\n\nProse.\n"),
        ("legacy.html", "<p>Bytes, verbatim.</p>\n"),
        ("notes.txt", "Bytes, verbatim.\n"),
    ];
    support::site("io-facts", who, &files)
}

/// The fact itself, on both sides of the line the item was written to expose.
///
/// Mutation: change the posts loader's `front_mattered: raw.front_mattered` to
/// `true` (the value `rendered` takes there) and the second assertion fails —
/// `/blog/2020/01/02/bare/` joins the identity set. Change the tree loader's to
/// `false` and `/about/` leaves it. Each restored.
#[test]
fn front_mattered_is_identity_not_parsing() {
    let dir = site("identity");
    let identity = urls(&dir, "/identity.xml");
    assert_eq!(
        identity,
        ["/about/", "/blog/2020/01/01/hello/"],
        "a block in the file, and nothing else"
    );

    let bytes = urls(&dir, "/bytes.xml");
    for u in ["/legacy.html", "/notes.txt"] {
        assert!(bytes.contains(&u.to_string()), "a byte copy: {bytes:?}");
    }
    assert!(
        bytes.contains(&"/blog/2020/01/02/bare/".to_string()),
        "a post the scope parsed but the author gave no identity: {bytes:?}"
    );
    assert!(
        bytes.contains(&"/blog/".to_string()),
        "a view route has no source file, so it carried nothing: {bytes:?}"
    );
}

/// Why the corpus migration is a migration and not a rename.
///
/// `kind == "post" || kind == "page"` and `front_mattered` select the same set
/// on every site whose documents all carry blocks — which is every example
/// site, and why their search routes moved. They part company on exactly one
/// row shape, and grack.com has one: a `.md` in a posts scope with no block.
/// `kind == "post"` is **scope membership**; `front_mattered` is **identity**.
/// That is the whole reason grack.com's search route did not migrate here.
#[test]
fn the_old_spelling_and_the_new_one_part_company_on_the_blockless_post() {
    let dir = site("spelling");
    let old = urls(&dir, "/old-spelling.xml");
    let new = urls(&dir, "/identity.xml");
    assert_eq!(
        old,
        [
            "/about/",
            "/blog/2020/01/01/hello/",
            "/blog/2020/01/02/bare/"
        ],
        "the fossil selects by table tag"
    );
    let only_old: Vec<_> = old.iter().filter(|u| !new.contains(u)).collect();
    assert_eq!(only_old, ["/blog/2020/01/02/bare/"]);
    assert!(
        new.iter().all(|u| old.contains(u)),
        "identity is a subset of the tag, never the other way"
    );
}

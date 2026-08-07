//! Scope membership on the output pool: `collection` on the route.
//!
//! The `kind` enum is deleted. Its last two live filters (grack.com's search
//! route and the drafts-profile restatement) respelled to what they always
//! meant — which collection admitted the row — and that respelling stands on
//! the `collection` column being queryable where fold filters run. This file
//! pins that column at fixture scale.

use std::path::PathBuf;

mod support;

/// One corpus with every route shape beside the others: two posts, a
/// front-mattered page, a byte-copy `.txt`, a routed `.png` in an objects
/// scope, a paginated listing, a grouped archive, and two pool folds — one
/// unfiltered, one selecting the posts collection.
fn site(who: &str) -> PathBuf {
    let files: [(&str, &[u8]); 6] = [
        (
            "grackle.toml",
            b"[site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\n\
              [[collections]]\nname = \"posts\"\nsource = \"_posts\"\narchives = { tags = \"tags\" }\n\n\
              [[collections]]\nname = \"objects\"\n\n  \
              [[collections.rules]]\n  match = \"**/*.png\"\n  route = \"/{path}\"\n\n\
              [views.tags]\npath = \"/tags/{group:key}/\"\nfrom = \"posts\"\n\
              group_by = \"tags\"\nlayout = \"card\"\n\n\
              [views.paged]\npaths = [\"/journal/\", \"/journal/page/{n}/\"]\n\
              from = \"posts\"\nlayout = \"card\"\npaginate = 1\n\n\
              [views.everything]\npath = \"/everything.xml\"\nshell = \"sitemap\"\n\n\
              [views.blog_corpus]\npath = \"/blog-corpus.xml\"\nshell = \"sitemap\"\n\
              where = 'collection == \"posts\"'\n\n\
              [[collections]]\nname = \"entries\"\nsource = \".\"\n\n  \
              [[collections.rules]]\n  match = \"notes.txt\"\n  \
              route = \"/{path}\"\n  defaults = { shell = \"raw\" }\n",
        ),
        (
            "_posts/2020-01-01-hello.md",
            b"---\ntitle: Hello\ntags: [rust]\n---\n\nProse.\n",
        ),
        (
            "_posts/2020-01-02-again.md",
            b"---\ntitle: Again\ntags: [rust]\n---\n\nMore prose.\n",
        ),
        ("about.md", b"---\ntitle: About\n---\n\nProse.\n"),
        ("notes.txt", b"Bytes, verbatim.\n"),
        // A 2x3 PNG, the shape the objects tests use.
        (
            "assets/kite.png",
            &[
                0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
                0x44, 0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x08, 0x06, 0x00, 0x00,
                0x00, 0xB8, 0x1F, 0x93, 0x21, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78,
                0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
                0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
            ],
        ),
    ];
    support::site("io-scope", who, &files)
}

/// Every route in the load table is exactly one of row-backed or view-minted
/// — the split every render pass dispatches on now that `kind` is gone. A
/// view route that forgot its `view` column would fall silently out of every
/// pass that selects on it; a row route that gained one would render as a
/// listing. Both sides witnessed, because a biconditional over an empty set
/// is true and says nothing.
#[test]
fn every_route_is_row_backed_xor_view_minted() {
    let dir = site("split");
    let db = support::load(&dir);
    for r in &db.routes {
        assert!(
            r.row.is_some() != r.view.is_some(),
            "{}: row {:?} view {:?}",
            r.url,
            r.row,
            r.view
        );
    }
    assert!(db.routes.iter().any(|r| r.row.is_some()));
    assert!(db.routes.iter().any(|r| r.view.is_some()));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The blog corpus fold selects posts-collection outputs and nothing else.
///
/// Mutation: delete `s.insert("collection", Str)` from `route_schema` and the
/// site stops loading on an unknown field.
#[test]
fn collection_on_the_route_selects_the_blog_corpus() {
    let dir = site("column");
    let (out, _) = support::built(&dir);

    let locs = |route: &str| -> Vec<String> {
        support::sitemap_locs(
            &String::from_utf8(out.get(route).expect("the fold published").clone())
                .expect("a sitemap is utf-8"),
        )
    };

    assert_eq!(
        locs("/blog-corpus.xml"),
        ["/blog/2020/01/01/hello/", "/blog/2020/01/02/again/"],
        "scope membership, and nothing else"
    );
    // The set it is NOT: every output — what any narrower column would have
    // had to be carved from, and the pool the unfiltered fold still reads.
    let all = locs("/everything.xml");
    for u in ["/about/", "/notes.txt", "/assets/kite.png", "/blog/"] {
        assert!(all.contains(&u.to_string()), "the pool holds {u}: {all:?}");
    }

    let _ = std::fs::remove_dir_all(&dir);
}

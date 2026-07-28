//! What is left of the `kind` enum, and the one equality every I13 respelling
//! stands on (IO.md §3, I13 — *delete `kind`*).
//!
//! I13 replaced eight `kind == View` tests across `build.rs`, `links.rs`,
//! `trails.rs`, `load.rs` and `views.rs` with the `view` column — three of them
//! were asking the same question twice and were deleted outright. Every one of
//! those rewrites is correct because of ONE property of the route table:
//!
//! > a route's `kind` is `View` **iff** its `view` column is non-empty.
//!
//! That property is held by nothing but the three sites in `views.rs` that mint
//! a view route (each sets both) and the row-route constructor in `load.rs`
//! (which sets neither). Nothing checks it, and if it ever stops being true the
//! eight respellings go wrong silently and in different directions. So it is
//! checked here, once, over a site carrying every route shape at the same time.
//!
//! The other two assertions are the item's CENSUS, pinned rather than written
//! down: what the enum still carries that facts do not, and what it carries
//! that they do.

use std::path::{Path, PathBuf};

use grackle_model::RouteKind;

/// One corpus with every route shape beside the others, because the claim is
/// about the table as a whole rather than about any one row:
///
/// - two **posts** (a posts scope) — `Post`;
/// - a front-mattered `.md` in the tree — `Page`;
/// - a `.txt` — `Static`, a byte copy;
/// - a routed `.png` in an objects scope — `Object`;
/// - a paginated blog listing — `View` routes wearing a `page`;
/// - a grouped tag archive — `View` routes wearing a `key`;
/// - a from-less sitemap fold over the output pool — a `View` route with no
///   members of its own;
/// - and the probe the surviving COLUMN is measured through: a second fold
///   filtered `kind == "post"`, which is the spelling grack.com's search route
///   and its drafts restatement carry and the one I13 could not take.
fn site(who: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("grackle-io-kind-{who}"));
    let _ = std::fs::remove_dir_all(&dir);
    let files: [(&str, &[u8]); 6] = [
        (
            "grackle.toml",
            b"[site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\n\
              [[collections]]\nname = \"objects\"\n\n  \
              [[collections.rules]]\n  match = \"**/*.png\"\n  route = \"/{path}\"\n\n\
              [routes.tags]\npath = \"/tags/{group:key}/\"\nfrom = \"posts\"\n\
              group_by = \"tags\"\nlayout = \"card\"\n\n\
              [routes.paged]\npaths = [\"/journal/\", \"/journal/page/{n}/\"]\n\
              from = \"posts\"\nlayout = \"card\"\npaginate = 1\n\n\
              [routes.everything]\npath = \"/everything.xml\"\nshell = \"sitemap\"\n\n\
              [routes.blog_corpus]\npath = \"/blog-corpus.xml\"\nshell = \"sitemap\"\n\
              where = 'kind == \"post\"'\n",
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
    for (rel, body) in files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().expect("a file has a directory")).unwrap();
        std::fs::write(&p, body).unwrap();
    }
    dir
}

fn load(dir: &Path) -> grackle::db::SiteDb {
    let cfg = grackle::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    grackle_source::load(&cfg).expect("the site loads")
}

/// **The equality every I13 respelling stands on.**
///
/// Checked in both directions and with both sides witnessed, because a
/// biconditional over an empty set is true and says nothing: the site has view
/// routes of three shapes (paginated, grouped, from-less fold) and row routes
/// of all four (post, page, static, object), and the assertion is over the
/// whole table.
///
/// Mutations, each red and each restored:
/// - drop `view: Some(name.clone())` from `build_pool_folds` in `views.rs` —
///   `/everything.xml` becomes a `View` route the `view` column cannot see, and
///   every one of the eight respellings starts answering "not a view" for it.
///   (The half-built symptom in the wild: `trails.rs` stops finding the
///   landing, `links.rs` suggests a source path for a fold, and `load.rs`'s
///   claim resolution skips it.)
/// - give the row-route constructor in `load.rs` a `view: Some(…)` — the other
///   direction, a row route the respellings would treat as a listing.
#[test]
fn a_view_route_is_exactly_a_route_that_names_a_view() {
    let dir = site("biconditional");
    let db = load(&dir);

    for r in &db.routes {
        assert_eq!(
            r.kind == RouteKind::View,
            r.view.is_some(),
            "kind and the view column must agree: {} is {:?} with view {:?}",
            r.url,
            r.kind,
            r.view
        );
    }

    // Both sides witnessed, and every row-route kind present — otherwise the
    // loop above proves whatever the table happens to hold.
    let seen: std::collections::BTreeSet<&str> =
        db.routes.iter().map(|r| r.kind.as_str()).collect();
    for k in ["post", "page", "static", "object", "view"] {
        assert!(
            seen.contains(k),
            "the corpus is missing a {k} route: {seen:?}"
        );
    }
    let views = db.routes.iter().filter(|r| r.view.is_some()).count();
    assert!(views >= 3, "three view shapes at least, got {views}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// **What the enum carries that the facts DO carry** — the measurement behind
/// the respelling I13 declined at `build.rs`'s render dispatch.
///
/// `Static | Object` is one arm there and `Page` is another, and the reason the
/// two arms differ is not the enum: it is the rendering law (IO.md §4,
/// `front_mattered || shell ∈ DOCUMENT`). Every `Static` and every `Object`
/// route's row is `rendered false`; every `Page` and every `Post` route's row is
/// `rendered true`. Measured on all six corpus trees at I13 and pinned here at
/// fixture scale, so the declined option stays available rather than becoming
/// folklore.
///
/// Mutation: none is owed — this asserts an agreement, not a guard. What it
/// protects is the log entry: if this ever goes red, `build.rs`'s dispatch has
/// stopped being respellable and the reason has changed.
#[test]
fn the_byte_copy_arms_are_exactly_the_rows_that_do_not_render() {
    let dir = site("rendered");
    let db = load(&dir);

    for r in &db.routes {
        let Some(p) = r.row.as_ref().and_then(|k| db.rows.get(k)) else {
            assert_eq!(r.kind, RouteKind::View, "only a fold has no row: {}", r.url);
            continue;
        };
        let renders = matches!(r.kind, RouteKind::Post | RouteKind::Page);
        assert_eq!(
            renders, p.rendered,
            "{}: kind {:?} vs rendered {}",
            r.url, r.kind, p.rendered
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// **What the enum carries that the facts do NOT** — and therefore why the
/// column survives I13 whole.
///
/// `kind == "post"` means SCOPE MEMBERSHIP, which the output pool has no other
/// column for: `front_mattered` is identity (it would admit `/about/`), `shell`
/// is serialization (it would admit every document), and `collection` is a row
/// column that never reached the route. This is grack.com's live spelling, in
/// its search route and its `[profiles.drafts]` restatement, and I13 could not
/// migrate it — so the column, its `Enum` domain and the `kind` line
/// `grackle explain` prints for an output all stay.
///
/// Mutation: delete `s.insert("kind", Enum(RouteKind::NAMES))` from
/// `route_schema` and the site stops loading on an unknown field — which is the
/// day this filter would have needed its replacement spelling to exist.
#[test]
fn the_surviving_column_still_selects_the_blog_corpus() {
    let dir = site("column");
    let cfg = grackle::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    let mut db = grackle_source::load(&cfg).expect("the site loads");
    let (out, _) = grackle::build::render_site(&cfg, &mut db).expect("the site renders");
    let _ = std::fs::remove_dir_all(dir.join("_cache"));

    let locs = |route: &str| -> Vec<String> {
        String::from_utf8(out.get(route).expect("the fold published").clone())
            .expect("a sitemap is utf-8")
            .lines()
            .filter_map(|l| l.strip_prefix("<loc>")?.strip_suffix("</loc>"))
            .map(|u| u.trim_start_matches("https://example.com").to_string())
            .collect()
    };

    assert_eq!(
        locs("/blog-corpus.xml"),
        ["/blog/2020/01/01/hello/", "/blog/2020/01/02/again/"],
        "scope membership, and nothing else"
    );
    // The set it is NOT: every output, which is what any facts-only spelling
    // available today would have had to narrow from.
    let all = locs("/everything.xml");
    for u in ["/about/", "/notes.txt", "/assets/kite.png", "/blog/"] {
        assert!(all.contains(&u.to_string()), "the pool holds {u}: {all:?}");
    }

    let _ = std::fs::remove_dir_all(&dir);
}

//! Extension selection is rules (IO.md I7a).
//!
//! `[[collections]] extensions = [...]` is gone. What an objects scope claims
//! is what its RULES claim — `match = "**/*.{png,jpg}"` — so the one mechanism
//! that already says where a row lands now also says whether the row is an
//! object at all. `is_obj`'s pre-rule extension scan is a glob test.
//!
//! Sites rather than unit tests, and BUILT rather than only loaded, because
//! the subject is what falls out of membership rather than membership itself:
//! an object gets a header read, an index entry and its scope's routing; the
//! same file as a tree row gets a byte copy at an eager URL. A test that asked
//! the loader "is this an object" would pass against an engine that then
//! published the file anyway.

use grackle::db::{Row, SiteDb};
use std::path::{Path, PathBuf};

/// A 2×3 PNG — real bytes, because half of what an object gets over a byte
/// copy is the header read that fills `width`/`height`.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x08, 0x02, 0x00, 0x00, 0x00, 0x36, 0x88, 0x49,
    0xd6, 0x00, 0x00, 0x00, 0x16, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x60, 0x10, 0x91, 0x03,
    0x21, 0x2e, 0x11, 0x39, 0x2e, 0x10, 0x4b, 0x44, 0x0e, 0x88, 0x00, 0x0d, 0x49, 0x01, 0x69, 0x37,
    0x8b, 0x8f, 0x82, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// Write a site and hand back its directory.
fn site(who: &str, files: &[(&str, &[u8])]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("grackle-io-objects-{who}"));
    let _ = std::fs::remove_dir_all(&dir);
    for (rel, body) in files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().expect("a file has a directory")).unwrap();
        std::fs::write(&p, body).unwrap();
    }
    dir
}

fn load(dir: &Path) -> SiteDb {
    let cfg = grackle::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    grackle_source::load(&cfg).expect("the site loads")
}

fn render(dir: &Path) -> grackle::build::SiteOutput {
    let cfg = grackle::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    let mut db = grackle_source::load(&cfg).expect("the site loads");
    let (out, _) = grackle::build::render_site(&cfg, &mut db).expect("the site renders");
    let _ = std::fs::remove_dir_all(dir.join("_cache"));
    out
}

/// Every URL the build publishes — the set `grackle urls` prints.
fn published(dir: &Path) -> Vec<String> {
    let mut urls: Vec<String> = render(dir).keys().cloned().collect();
    urls.sort();
    urls
}

/// The `/gallery/` links the listing at `route` carries.
fn members(dir: &Path, route: &str) -> Vec<String> {
    let out = render(dir);
    let page = String::from_utf8(
        out.get(route)
            .unwrap_or_else(|| panic!("no listing at {route} — routes: {:?}", out.keys()))
            .clone(),
    )
    .expect("the listing is utf-8");
    page.split("href=\"")
        .skip(1)
        .filter_map(|s| s.split('"').next())
        .filter(|h| h.starts_with("/gallery/"))
        .map(|h| h.to_string())
        .collect()
}

/// The row at `rel`, whatever scope claimed it.
fn row<'a>(db: &'a SiteDb, rel: &str) -> &'a Row {
    db.rows
        .iter()
        .find(|r| r.rel.to_string_lossy() == rel)
        .unwrap_or_else(|| panic!("no row at {rel}"))
}

/// grack.com's shape, minimised: an objects scope claiming images ON DEMAND,
/// and a tree catch-all publishing everything else verbatim at its literal
/// path. The two rules overlap on every image, which is the point — precedence
/// decides, and precedence now reads the objects rule's glob.
fn case_config(glob: &str) -> String {
    format!(
        "extends = \"none\"\n\
         [site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\n\
         [[collections]]\nname = \"objects\"\nkind = \"objects\"\n\n\
         [[collections.rules]]\nmatch = \"{glob}\"\nroute = \"/{{path}}\"\non_demand = true\n\n\
         [[collections]]\nkind = \"tree\"\nname = \"entries\"\nsource = \".\"\n\n\
         [[collections.rules]]\nmatch = \"**/*\"\nroute = \"/{{path}}\"\n"
    )
}

/// **The .PNG pin.** `assets/2004/06/after-theme-hack.PNG` is the one row in
/// the whole corpus that can tell a case-sensitive glob from a
/// case-insensitive one, and it is an image on a published page of a live
/// site.
///
/// Before I7a, membership lowercased the extension before comparing, so case
/// never reached the decision. Moving the decision into a glob hands it to
/// globset, which is case-SENSITIVE by default — so rule globs compile
/// `case_insensitive(true)`, and this is what that buys.
///
/// Both halves are asserted because losing either is a different bug: the
/// upper-cased file must be an OBJECT (the header read, the objects index, its
/// scope's on-demand routing), and its literal path must therefore NOT be
/// published, since the eager tree catch-all is what claims it otherwise.
///
/// Mutation: drop `.case_insensitive(true)` in `compile_rules` and this fails
/// on both — `UPPER.PNG` is claimed by the tree, arrives with no dimensions,
/// and `/assets/UPPER.PNG` joins the published set. Measured on a real build of
/// grack.com too, which is where it matters: the same mutation drops that one
/// file out of the object set (838 → 837) and mints
/// `/assets/2004/06/after-theme-hack.PNG` as an eager URL that no rule asked
/// for.
#[test]
fn an_upper_cased_extension_is_claimed_by_the_same_glob() {
    let dir = site(
        "case",
        &[
            ("grackle.toml", case_config("**/*.{png,jpg}").as_bytes()),
            ("assets/lower.png", PNG),
            ("assets/UPPER.PNG", PNG),
            ("notes.txt", b"Bytes.\n"),
        ],
    );
    let db = load(&dir);
    for rel in ["assets/lower.png", "assets/UPPER.PNG"] {
        let r = row(&db, rel);
        assert_eq!(r.collection, "objects", "{rel} left the objects scope");
        assert_eq!(
            (r.width, r.height),
            (Some(2), Some(3)),
            "{rel} lost its header read"
        );
    }
    assert_eq!(
        db.object_ix.len(),
        2,
        "both images are in the objects index"
    );

    let urls = published(&dir);
    assert!(
        urls.contains(&"/notes.txt".to_string()),
        "the tree catch-all still publishes non-images: {urls:?}"
    );
    for u in ["/assets/lower.png", "/assets/UPPER.PNG"] {
        assert!(
            !urls.contains(&u.to_string()),
            "{u} was published, so the objects scope did not claim it: {urls:?}"
        );
    }
}

/// The control for the pin, in the direction a widening mistake hides in: an
/// extension the glob does NOT name is still not an object, whatever its case.
/// A case-insensitive glob has to be indifferent to the shift key and to
/// nothing else — a refusal that is too wide is the mistake available here,
/// and only a control catches it.
///
/// Mutation: widen the objects rule to `**/*` and this fails — `notes.TXT`
/// joins the object set and stops being published at its own path.
#[test]
fn case_insensitivity_widens_nothing_but_the_case() {
    let dir = site(
        "control",
        &[
            ("grackle.toml", case_config("**/*.{png,jpg}").as_bytes()),
            ("assets/UPPER.PNG", PNG),
            ("notes.TXT", b"Bytes.\n"),
        ],
    );
    let db = load(&dir);
    assert_eq!(row(&db, "assets/UPPER.PNG").collection, "objects");
    assert_eq!(row(&db, "notes.TXT").collection, "entries");
    assert!(published(&dir).contains(&"/notes.TXT".to_string()));
}

/// **One extension leaving a glob empties a gallery.** grack.com's
/// `demos/mindstorms/` is a hundred `.jpg` files claimed by an objects rule and
/// listed by a view over that scope; this is that shape, minimised.
///
/// The mutation is this item's own edit run backwards — delete `jpg` from the
/// rule's glob — and what makes it worth a test is how QUIET it is: the files
/// are still published, at the same URLs, with the same bytes, by the tree's
/// catch-all. Only membership moves, so only a query over the scope can see
/// it. That is the whole argument for asserting the listing's members rather
/// than the build's file list — and the second half of the test asserts the
/// silence too, so a reader knows the byte set is not the guard.
#[test]
fn one_extension_leaving_the_glob_empties_the_gallery() {
    let cfg = |exts: &str| {
        format!(
            "extends = \"none\"\n\
             [site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\n\
             [[collections]]\nname = \"objects\"\nkind = \"objects\"\n\n\
             [[collections.rules]]\nmatch = \"gallery/**/*.{exts}\"\nroute = \"/{{path}}\"\n\n\
             [[collections]]\nkind = \"tree\"\nname = \"entries\"\nsource = \".\"\n\n\
             [[collections.rules]]\nmatch = \"**/*\"\nroute = \"/{{path}}\"\n\n\
             [routes.gallery]\npath = \"/photos/\"\nfrom = \"objects\"\n\
             order_by = \"name\"\nlayout = \"listing\"\ntitle = \"Photos\"\n"
        )
    };
    let dir = site(
        "gallery",
        &[
            ("grackle.toml", cfg("{png,jpg}").as_bytes()),
            ("gallery/one.jpg", PNG),
            ("gallery/two.jpg", PNG),
        ],
    );
    assert_eq!(load(&dir).object_ix.len(), 2, "both jpgs are objects");
    assert_eq!(
        members(&dir, "/photos/"),
        vec!["/gallery/one.jpg", "/gallery/two.jpg"],
        "the gallery lists what the objects scope claimed"
    );

    std::fs::write(dir.join("grackle.toml"), cfg("{png}")).unwrap();
    assert_eq!(
        load(&dir).object_ix.len(),
        0,
        "with `jpg` gone from the glob nothing claims the gallery"
    );
    assert_eq!(
        members(&dir, "/photos/"),
        Vec::<String>::new(),
        "the gallery is empty"
    );
    // The quiet half: both files still ship, at the same URLs, as ordinary
    // byte copies. Nothing in the output says the gallery lost its members.
    let urls = published(&dir);
    for u in ["/gallery/one.jpg", "/gallery/two.jpg"] {
        assert!(urls.contains(&u.to_string()), "{u} still ships: {urls:?}");
    }
}

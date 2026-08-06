//! Objects dissolve; the Null shape collapses.
//!
//! There is one row constructor now. A former-object row takes rule defaults,
//! marker defaults, schema validation and rung 0 like every other row, and what
//! survives of "object" is a fact about the file, the objects scopes' globs,
//! rule-claimed membership, read by the loader as **the extension fact**.
//! Two things key off it and nothing else does: `by_name`, and the header
//! read that fills `width`/`height`.
//!
//! Built sites rather than loaded ones wherever the claim is about what a
//! reader DOES with the index: the listing pass asks `by_name` whether a
//! member is a picture, and a test that asked the database the same question
//! would pass against an engine whose gallery had stopped showing pictures.

use std::path::{Path, PathBuf};

mod support;

/// A 2×3 PNG, real bytes, because one of the three readers under test is the
/// header read.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x08, 0x02, 0x00, 0x00, 0x00, 0x36, 0x88, 0x49,
    0xd6, 0x00, 0x00, 0x00, 0x16, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x60, 0x10, 0x91, 0x03,
    0x21, 0x2e, 0x11, 0x39, 0x2e, 0x10, 0x4b, 0x44, 0x0e, 0x88, 0x00, 0x0d, 0x49, 0x01, 0x69, 0x37,
    0x8b, 0x8f, 0x82, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

fn site(who: &str, files: &[(&str, &[u8])]) -> PathBuf {
    support::site("io-dissolve", who, files)
}

use support::{load, render};

/// One page of a built site, as text.
fn page(dir: &Path, url: &str) -> String {
    let out = render(dir);
    String::from_utf8(
        out.get(url)
            .unwrap_or_else(|| panic!("no page at {url} — routes: {:?}", out.keys()))
            .clone(),
    )
    .expect("a page is utf-8")
}

/// The URLs a fold selected, out of its sitemap.
fn selected(dir: &Path, route: &str) -> Vec<String> {
    support::sitemap_locs(&page(dir, route))
}

/// **The re-keying, and what it buys.** `by_name` and the header read key off
/// the extension fact rather than off the vector a row arrived in.
///
/// All are asserted together because they are one decision, and each of
/// them is a different consumer's whole answer:
///
/// - `by_name` is what `grackle query stats` reports distinct and ambiguous
/// names from, the measurement collision argument rests on, which is
///  why the fixture carries two `shot.png`s in different directories;
/// - `by_name` is also what the listing pass asks per member (`ctx.objects`),
///  and an object member previews as a PICTURE while a row member previews as
///  prose, so the gallery is where losing the index shows;
/// - `width`/`height` come from a header read the loader only performs on the
///  rows this fact names.
///
/// Mutation: send former-object rows to `pages` instead (`(_, true) =>
/// objects.push(row)` -> `pages.push(row)`), the index empties, `by_name` goes
/// with it (distinct 0, ambiguous 0), the header read never runs, and the
/// gallery comes out with three members and no links at all, because a picture
/// answers `title` with nothing and the empty label takes its `<a>` with it.
/// The MEMBERSHIP half survives that mutation on purpose, the row's
/// `collection` is still the objects scope's, so the view still selects it,
/// which is what makes every assertion here about the INDEX rather than about
/// the query.
#[test]
fn the_objects_index_keys_off_the_extension_fact() {
    let dir = site(
        "index",
        &[
            (
                "grackle.toml",
                b"extends = \"none\"\n\
                  [site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\n\
                  [[collections]]\nname = \"objects\"\n\n\
                  [[collections.rules]]\nmatch = \"gallery/**/*.png\"\nroute = \"/{path}\"\n\n\
                  [[collections]]\nname = \"entries\"\nsource = \".\"\n\n\
                  [[collections.rules]]\nmatch = \"**/*\"\nroute = \"/{path}\"\n\n\
                  [routes.gallery]\npath = \"/photos/\"\nfrom = \"objects\"\n\
                  order_by = \"path\"\nlayout = \"card\"\ntitle = \"Photos\"\n",
            ),
            ("gallery/a/shot.png", PNG),
            ("gallery/b/shot.png", PNG),
            ("gallery/solo.png", PNG),
            ("notes.txt", b"Bytes.\n"),
        ],
    );

    // What the index BUYS, asserted first: the listing pass asks `by_name`
    // per member, and an object previews as the picture it is, labelled by its
    // stem, which is the only name a file has. A row member has a `title` and
    // an image has none, so losing the index does not mis-label the gallery, it
    // empties it: rule 2 deletes an element with an empty content slot, so the
    // link goes with the label.
    let gallery = page(&dir, "/photos/");
    assert_eq!(
        gallery.matches("href=\"/gallery/").count(),
        3,
        "each member previews as the picture it is:\n{gallery}"
    );
    assert!(
        gallery.contains(">solo</a>"),
        "and its stem is the label:\n{gallery}"
    );

    let db = load(&dir);
    assert_eq!(
        db.by_name.values().map(|v| v.len()).sum::<usize>(),
        3,
        "three pictures"
    );
    assert_eq!(
        db.by_name.len(),
        2,
        "distinct names: `shot.png` collides with itself, `solo.png` does not"
    );
    assert_eq!(
        db.by_name.values().filter(|v| v.len() > 1).count(),
        1,
        "ambiguous names — the count `query stats` prints"
    );
    for o in db.by_name.values().flatten().filter_map(|k| db.rows.get(k)) {
        assert_eq!(
            (o.width, o.height),
            (Some(2), Some(3)),
            "{} lost its header read",
            o.rel.display()
        );
    }
    // The `.txt` is the control: it is a row of the same walk, built by the
    // same constructor, and it is in none of the three.
    assert!(
        db.by_name.values().flatten().count() == 3 && !db.by_name.contains_key("notes.txt"),
        "the name index is the pictures, not the corpus"
    );
}

/// **Markers reach a former-object row**, the propose-and-flag call. A `.hidden` beside a gallery means what it says: refusing it would
/// re-mint the origin distinction this item deletes, since the only reason to
/// refuse is *which constructor built the row*.
///
/// Asserted through a fold over the route pool, because the point is that the
/// fact reaches the output side where a query can act on it, a marker that
/// wrote a field nothing could read would be the declared-and-ignored disease
/// with extra steps. (The gallery's own vocabulary is deliberately narrower:
/// `where = "hidden"` on an object view is still a load error, which is the
/// other half of and is not disturbed here.)
///
/// Byte-inert on the corpus today, and that was measured rather than assumed:
/// `query stats` reports 0 marker files on all five sites, so no image on any
/// of them sits under one.
///
/// The fixture declares its own objects route, because the base routes no
/// image now, and the assertion is about a marker reaching a row,
/// not about where the row lands, so the site says where in one line the way
/// every corpus site with images does.
///
/// Mutation: pass `&Defaults::default()` in place of `marker_defaults` when
/// `object_shaped`, the origin distinction re-minted in one line, which is
/// exactly what this test refuses, and the image leaves the set while the
/// `.md` beside it stays.
#[test]
fn a_marker_reaches_a_former_object_row() {
    let dir = site(
        "marker",
        &[
            (
                "grackle.toml",
                b"[site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\n\
                  [[collections]]\nname = \"objects\"\n\
                  [[collections.rules]]\n\
                  match = \"**/*.{png,jpg,jpeg,gif,webp,svg}\"\nroute = \"/{path}\"\n\n\
                  [routes.hidden_probe]\npath = \"/hidden.xml\"\n\
                  shell = \"sitemap\"\nwhere = 'hidden'\n",
            ),
            ("gallery/.hidden", b""),
            ("gallery/private.png", PNG),
            ("gallery/notes.md", b"---\ntitle: Notes\n---\n\nProse.\n"),
            ("public.png", PNG),
        ],
    );
    let hidden = selected(&dir, "/hidden.xml");
    assert!(
        hidden.contains(&"/gallery/private.png".to_string()),
        "the marker's defaults reach the image beneath it: {hidden:?}"
    );
    assert!(
        hidden.contains(&"/gallery/notes/".to_string()),
        "and the page beside it, as they always did: {hidden:?}"
    );
    assert!(
        !hidden.contains(&"/public.png".to_string()),
        "a marker governs its own directory and no other: {hidden:?}"
    );
    // The same fact on the row, so a reader knows the route did not invent it.
    let db = load(&dir);
    // Found through `rows` rather than `objects()`, so this test says nothing
    // about the index, it is about what the constructor put on the row.
    let img = db
        .rows
        .iter()
        .find(|o| o.rel.to_string_lossy() == "gallery/private.png")
        .expect("the image is a row");
    assert_eq!(
        img.fields.get("hidden"),
        Some(&grackle_core::filter::Value::Bool(true)),
        "the row carries the declared field the marker set"
    );
}

/// **An objects rule does not spend locale in `file`**, so `photo.fr.png` keeps
/// the dots it was given. One picture serves every locale; a
/// name that happens to carry `.fr.` is not a translation unless a `file`
/// pattern says so.
///
/// Both halves are here because the claim is about the rule, not the site: the
/// `.md` beside it declares `{stem}.{axis:locale}` and IS the French edition,
/// at the prefixed URL. The fixture routes its own images (the base no
/// longer does), because "the image keeps the name it was given" is a claim
/// about an address and an unrouted asset has none to keep.
///
/// Mutation: put `file = ["{stem}.{axis:locale}", "{stem}"]` on the objects
/// rule, the image is republished at `/fr/gallery/photo.png` and its literal
/// path disappears from the URL set. (The reverse mutation, omitting `file`
/// on the tree, takes the French page's URL with it.)
#[test]
fn an_image_is_not_a_translation_of_itself() {
    let dir = site(
        "locale",
        &[
            (
                "grackle.toml",
                b"[site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\n\
                  [[collections]]\nname = \"objects\"\n\
                  [[collections.rules]]\n\
                  match = \"**/*.{png,jpg,jpeg,gif,webp,svg}\"\nroute = \"/{path}\"\n\n\
                  [[collections]]\nsource = \".\"\n\
                  file = [\"{stem}.{axis:locale}\", \"{stem}\"]\n\n\
                  [[collections.rules]]\n\
                  match = \"**/*.{html,md}\"\nfront_matter = true\n\
                  route = [\"/{axis:locale}/{dir}/{stem}/\", \"/{dir}/{stem}/\"]\n\n\
                  [axes.locale]\nvalues = [\"en\", \"fr\"]\nfield = \"locale\"\n\n\
                  [routes.all]\npath = \"/all.xml\"\nshell = \"sitemap\"\n",
            ),
            ("gallery/photo.fr.png", PNG),
            ("gallery/notes.md", b"---\ntitle: Notes\n---\n\nEnglish.\n"),
            (
                "gallery/notes.fr.md",
                "---\ntitle: Notes\n---\n\nFrançais.\n".as_bytes(),
            ),
        ],
    );
    let urls = selected(&dir, "/all.xml");
    assert!(
        urls.contains(&"/gallery/photo.fr.png".to_string()),
        "the image keeps the name it was given: {urls:?}"
    );
    assert!(
        !urls.iter().any(|u| u.contains("/fr/gallery/photo")),
        "and mints no locale address for a file nobody translated: {urls:?}"
    );
    assert!(
        urls.contains(&"/fr/gallery/notes/".to_string()),
        "while the page beside it IS a translation, at the prefixed URL: {urls:?}"
    );

    let db = load(&dir);
    let img = db
        .rows
        .iter()
        .find(|o| o.rel.to_string_lossy() == "gallery/photo.fr.png")
        .expect("the image is a row");
    assert_eq!(
        img.string("locale").unwrap_or(""),
        "en",
        "an image carries the default locale"
    );
}

// ---------------------------------------------------------------------------
// The corner closes.
//
// This was once documented at the code rather than guarded: "an objects rule
// gated `front_matter = true` would be claimed by whichever scope came next
// while still being indexed as a picture". That leaned on a premise since
// retired, that an object is never peeked, so the gate could never pass —
// but sidecars are identity a `.png` can have. The gate reads identity
// (`apply_rules(…, has_identity)`), so the corner is reachable, and it is
// refused where the author wrote it.

/// What `Config::load` said, flattened.
fn config_err(dir: &Path) -> String {
    format!(
        "{:#}",
        grackle_core::config::Config::load(&dir.join("grackle.toml"))
            .expect_err("the config must not load")
    )
}

/// **An objects rule selects by shape; the identity gate belongs to scopes
/// that parse**, and the refusal is on the KEY, not on one of its
/// values, so both spellings are here.
///
/// The fixture is the corner itself: a sidecar'd `photo.png` and a blockless
/// `plain.png` under one objects scope whose rule gates on front matter, with
/// a tree catch-all beneath to catch whatever the gate turns away.
///
/// Mutation, measured on this fixture rather than reasoned: delete the
/// `check_objects_rule_gate` call and both sites LOAD, one directory of two
/// pictures split between two scopes by whether someone wrote a `.toml`.
///
/// | rule | `photo.png` (sidecar'd) | `plain.png` (blockless) |
/// |---|---|---|
/// | `front_matter = true` | `objects`, `/pics/photo/` | `entries`, `/plain.png` |
/// | `front_matter = false` | `entries`, `/photo.png` | `objects`, `/pics/plain/` |
///
/// …and in BOTH runs `query stats` reports **objects 2, distinct names 2**,
/// because the extension fact never asked about identity. A row that `explain`
/// calls `collection entries` while the objects index counts it a picture is
/// the disagreement this refuses.
///
/// Two narrowings are checked too: widening the check to every kind kills the
/// control below, and narrowing it to `Some(true)` kills the second half of
/// this loop.
#[test]
fn an_objects_rule_may_not_declare_front_matter() {
    for want in ["true", "false"] {
        let dir = site(
            &format!("gate-{want}"),
            &[
                (
                    "grackle.toml",
                    format!(
                        "extends = \"none\"\n\n\
                         [site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\n\
                         [schema]\nshell = {{ type = \"string\" }}\n\n\
                         [[collections]]\nname = \"objects\"\n\n  \
                         [[collections.rules]]\n  match = \"**/*.png\"\n  \
                         front_matter = {want}\n  route = \"/pics/{{stem}}/\"\n  \
                         defaults = {{ shell = \"raw\" }}\n\n\
                         [[collections]]\nname = \"entries\"\n\
                         source = \".\"\n\n  \
                         [[collections.rules]]\n  match = \"**/*\"\n  route = \"/{{path}}\"\n  \
                         defaults = {{ shell = \"raw\" }}\n"
                    )
                    .as_bytes(),
                ),
                ("photo.png", PNG),
                ("photo.png.toml", b"title = \"A described picture\"\n"),
                ("plain.png", PNG),
            ],
        );
        let e = config_err(&dir);
        assert!(e.contains("\"objects\""), "names the collection: {e}");
        assert!(e.contains("\"**/*.png\""), "names the rule: {e}");
        assert!(
            e.contains(&format!("front_matter = {want}")),
            "quotes what was written: {e}"
        );
        assert!(
            e.contains("selects by SHAPE") && e.contains("scopes that PARSE"),
            "gives the reason: {e}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// The control the refusal owes: `front_matter` is a rule key like any other
/// wherever the scope PARSES its rows. A tree rule and a posts rule both gate
/// on it here, and both gates decide a route.
///
/// Mutation: widen `check_objects_rule_gate` to every kind and this site stops
/// loading, which is what "an objects rule" has to mean to be worth saying.
#[test]
fn the_front_matter_gate_still_works_where_a_scope_parses() {
    let dir = site(
        "gate-control",
        &[
            (
                "grackle.toml",
                b"[site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\n\
                  [[collections]]\nsource = \"_posts\"\n\n  \
                  [[collections.rules]]\n  match = \"**/*.md\"\n  front_matter = true\n  \
                  route = \"/blog/{slug}/\"\n  defaults = { shell = \"html\" }\n\n  \
                  [[collections.rules]]\n  match = \"**/*.md\"\n  route = \"/raw/{stem}\"\n  \
                  defaults = { shell = \"raw\" }\n\n\
                  [routes.all]\npath = \"/all.xml\"\nshell = \"sitemap\"\n",
            ),
            ("about.md", b"---\ntitle: About\n---\n\nProse.\n"),
            ("notes.md", b"Just bytes.\n"),
            (
                "_posts/2020-01-01-hello.md",
                b"---\ntitle: Hello\n---\n\nProse.\n",
            ),
            ("_posts/2020-01-02-bare.md", b"Just bytes.\n"),
        ],
    );
    let urls = selected(&dir, "/all.xml");

    // The tree's gate: the base's two rules, unmoved.
    assert!(urls.contains(&"/about/".to_string()), "{urls:?}");
    assert!(urls.contains(&"/notes.md".to_string()), "{urls:?}");
    // The posts scope's, written by this site.
    assert!(urls.contains(&"/blog/hello/".to_string()), "{urls:?}");
    assert!(
        urls.contains(&"/raw/2020-01-02-bare".to_string()),
        "{urls:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

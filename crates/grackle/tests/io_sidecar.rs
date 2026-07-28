//! Sidecars: identity for a file that cannot carry a block (IO.md I8).
//!
//! IO.md §1 says identity comes from "a literal block, or a sidecar file", and
//! §3 says what the second spelling buys: **sidecars split identity from
//! parsing**. A `.png` with a sidecar is a governed row — declared fields,
//! schema-validated, a title, a place in the link graph — whose bytes are never
//! parsed. Which facts a file has and what the pipeline does with it are
//! separate questions with separate answers, and this file is where the two
//! answers are pinned apart.
//!
//! Built sites rather than loaded ones wherever the claim is about what the
//! site PUBLISHES (the sidecar is not a row; the image still ships its bytes),
//! and loaded ones where the claim is about a fact no output can show (the
//! version fold, the warning that must not fire).
//!
//! No corpus site uses a sidecar, so parity is free and every guard here is the
//! capability's only evidence.

use grackle::db::SiteDb;
use grackle_db::Value;
use std::path::{Path, PathBuf};

/// A 2×3 PNG — real bytes, because a sidecar'd image is still an image: the
/// header read runs on it and a test that fed the loader text named `.png`
/// would not know if it stopped.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x08, 0x02, 0x00, 0x00, 0x00, 0x36, 0x88, 0x49,
    0xd6, 0x00, 0x00, 0x00, 0x16, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x60, 0x10, 0x91, 0x03,
    0x21, 0x2e, 0x11, 0x39, 0x2e, 0x10, 0x4b, 0x44, 0x0e, 0x88, 0x00, 0x0d, 0x49, 0x01, 0x69, 0x37,
    0x8b, 0x8f, 0x82, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// A site inheriting the base — so the objects rule, the tree rules and the
/// front-matter gate are the real ones rather than a fixture's idea of them.
/// The site routes its own images (IO.md I11: the base routes none), because
/// every claim below is about a governed image's own address — `/assets/
/// kite.png`, the bytes it ships there, the picture refusal that advises a
/// route — and an embed-addressed row has no address of its own to make those
/// claims about.
const CONFIG: &str = "[site]\nurl = \"https://example.com\"\ntitle = \"T\"\nauthor = \"A\"\n\
                      \n[schema]\nalt = { type = \"string\" }\ncover = { type = \"image\" }\n\
                      \n[[collections]]\nname = \"objects\"\nkind = \"objects\"\n\
                      [[collections.rules]]\n\
                      match = \"**/*.{png,jpg,jpeg,gif,webp,svg}\"\nroute = \"/{path}\"\n";

fn site(whose: &str, files: &[(&str, &[u8])]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("grackle-io-sidecar-{whose}"));
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

fn load_err(dir: &Path) -> String {
    let cfg = grackle::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    format!(
        "{:#}",
        grackle_source::load(&cfg).expect_err("the site must not load")
    )
}

fn render(dir: &Path) -> grackle::build::SiteOutput {
    let cfg = grackle::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    let mut db = grackle_source::load(&cfg).expect("the site loads");
    let (out, _) = grackle::build::render_site(&cfg, &mut db).expect("the site renders");
    let _ = std::fs::remove_dir_all(dir.join("_cache"));
    out
}

fn row<'a>(db: &'a SiteDb, rel: &str) -> &'a grackle::db::Row {
    db.rows
        .get(&grackle_db::Key::new(rel))
        .unwrap_or_else(|| panic!("no row for {rel}"))
}

/// **The headline** (IO.md §3): a `.png` with a sidecar is a governed row whose
/// bytes are never parsed.
///
/// Every clause of that sentence is a separate assertion, because they come
/// apart in different directions:
///
/// - **identity** — `front_mattered` true, from a file the image cannot hold;
/// - **not parsed** — `rendered` false, `body_bytes` 0, and the published bytes
///   are the PNG, byte for byte;
/// - **governed** — a declared field validates and lands on the row, and the
///   title the sidecar wrote is the row's title;
/// - **still a picture** — the extension fact is untouched, so the header read
///   still fills 2×3 and the objects index still holds it.
///
/// The two controls are in the same site: a sidecar-LESS image (identity false,
/// same bytes at the same URL) and an ordinary block-identity page. Neither may
/// move, and each fails a different wrong implementation — one that gave every
/// image identity, and one that broke blocks while adding sidecars.
///
/// Mutations, each red and each restored: drop the `sidecar.is_some()` term in
/// `has_identity` (the row loses identity, its title and its `alt`); pass
/// `has_identity` to `shell::renders` instead of the block — the split
/// collapsing, and the load dies one line later on the **picture refusal**,
/// which sits directly downstream of `rendered` and catches it first:
/// `assets/kite.png: shell = "raw" would render this file as a document, and
/// its bytes are a picture … Route it `raw``. The message advising a route the
/// row already wears is the collapse showing through, and it is the failure
/// mode to expect — not the UTF-8 error, which is what the *picture refusal's*
/// own mutation produces (`io_sidecar.rs`'s deferred-description test), one
/// rung further down.
#[test]
fn a_sidecar_gives_an_image_identity_without_parsing_it() {
    let dir = site(
        "identity",
        &[
            ("grackle.toml", CONFIG.as_bytes()),
            ("assets/kite.png", PNG),
            (
                "assets/kite.png.toml",
                b"title = \"A red kite\"\nalt = \"a red kite over a field\"\n",
            ),
            ("assets/plain.png", PNG),
            (
                "about.md",
                b"---\ntitle: About\n---\n\nA page with a block.\n",
            ),
        ],
    );
    let db = load(&dir);

    let kite = row(&db, "assets/kite.png");
    assert!(kite.front_mattered, "a sidecar is identity (IO.md §1)");
    assert!(
        kite.sidecar,
        "and `explain` can say which of the two it was"
    );
    assert!(
        !kite.rendered,
        "identity from a sidecar says nothing about the file's bytes — the \
         split IO.md §3 calls the feature"
    );
    assert_eq!(kite.body_bytes, 0, "nothing read the image as a body");
    assert_eq!(kite.title.as_deref(), Some("A red kite"));
    assert_eq!(
        kite.fields.get("alt"),
        Some(&Value::Str("a red kite over a field".into())),
        "a declared field validates and lands, exactly as it would from a block"
    );
    assert_eq!(
        (kite.width, kite.height),
        (Some(2), Some(3)),
        "the extension fact is untouched: it is still a picture"
    );
    assert!(
        db.object_ix.contains(&kite.key),
        "and still an object row (IO.md I7e's extension fact, not identity)"
    );

    // The control: the image beside it, with no sidecar, is what it was.
    let plain = row(&db, "assets/plain.png");
    assert!(!plain.front_mattered && !plain.sidecar && !plain.rendered);
    assert_eq!(plain.title, None);
    // And the control on the other side: a block still means what it meant.
    let about = row(&db, "about.md");
    assert!(about.front_mattered && !about.sidecar && about.rendered);
    assert_eq!(about.title.as_deref(), Some("About"));

    // What the site publishes: the PNG's own bytes at its own URL, unchanged
    // by having gained a name.
    let out = render(&dir);
    assert_eq!(
        out.get("/assets/kite.png").map(Vec::as_slice),
        Some(PNG),
        "a governed image still ships its bytes — `raw` is the transparent shell"
    );
    assert_eq!(out.get("/assets/plain.png").map(Vec::as_slice), Some(PNG));

    // IO.md I8's `explain` line: the fact carries its provenance, because
    // `true` beside `rendered false` is unreadable without it. And IO.md I9's:
    // a sidecar'd row's OUTPUT is its bytes — identity without content lands
    // exactly where the file always did, which is the third pair the block
    // teaches (`front_mattered true / rendered false / output <the file>`).
    assert_eq!(
        grackle::debug::row_facts(kite),
        "collection  objects\nrule        **/*.{png,jpg,jpeg,gif,webp,svg}\n\
         shell       raw\nfront_mattered true (sidecar)\nrendered    false\n\
         output      /assets/kite.png\n",
    );
    assert_eq!(
        grackle::debug::row_facts(about),
        "collection  entries\nrule        **/*.{html,md}\nshell       html\n\
         front_mattered true (block)\nrendered    true\noutput      /about/\n",
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// **A sidecar is governed like any block** (IO.md §5b): an undeclared key is a
/// load error naming the knowns — and it names the SIDECAR, which is the file
/// the author has to edit.
///
/// Mutation: name `f.path` instead of `identity_path` in the `validate` call
/// and the message sends its reader to the `.png`, a file with nothing in it
/// to fix.
#[test]
fn an_undeclared_key_in_a_sidecar_is_a_load_error_naming_the_knowns() {
    let dir = site(
        "governed",
        &[
            ("grackle.toml", CONFIG.as_bytes()),
            ("assets/kite.png", PNG),
            ("assets/kite.png.toml", b"caption = \"oops\"\n"),
        ],
    );
    let e = load_err(&dir);
    assert!(e.contains("\"caption\""), "{e}");
    assert!(e.contains("not declared"), "{e}");
    assert!(
        e.contains("kite.png.toml"),
        "the file to edit is the sidecar: {e}"
    );
    assert!(e.contains("alt, cover"), "the knowns are listed: {e}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// **Two sources of identity, no order between them** (IO.md I8, MERGE.md A5's
/// family): a file carrying a block AND wearing a sidecar is a load error.
///
/// The alternative was a precedence rule, and the argument against it is that
/// there is no honest one to write: a sidecar exists for files that CANNOT
/// carry a block, so a file doing both has said the same thing twice in two
/// places that will drift. It is the marker-collision shape — an unrankable
/// disagreement — and it gets the marker answer.
///
/// Mutation: delete the `bail!` and the site loads with the BLOCK silently
/// winning (the sidecar's title never appears), which is a precedence rule
/// nobody declared.
#[test]
fn a_block_and_a_sidecar_on_one_file_is_a_load_error() {
    let dir = site(
        "twice",
        &[
            ("grackle.toml", CONFIG.as_bytes()),
            ("about.md", b"---\ntitle: From the block\n---\n\nProse.\n"),
            ("about.md.toml", b"title = \"From the sidecar\"\n"),
        ],
    );
    let e = load_err(&dir);
    assert!(e.contains("identity twice"), "{e}");
    assert!(e.contains("about.md"), "{e}");
    assert!(e.contains("about.md.toml"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// **The sidecar is a declaration, not content** — no route, no bytes, not in
/// the URL set (IO.md I8, the statement `Markers::is_marker` makes one family
/// over).
///
/// The control is the whole reason the mechanism keys on the PAIR rather than
/// on the name: `netlify.toml` at the site root names no file beside it, so it
/// is an ordinary byte row and still ships. A rule spelled "a `.toml` is a
/// declaration" would have swallowed it silently.
///
/// Mutation: drop the `is_sidecar` filter in `walk_site` and
/// `/assets/kite.png.toml` joins the published set — the row's own metadata
/// served as a file.
#[test]
fn a_sidecar_is_not_a_row_and_a_lone_toml_still_is() {
    let dir = site(
        "notarow",
        &[
            ("grackle.toml", CONFIG.as_bytes()),
            ("assets/kite.png", PNG),
            ("assets/kite.png.toml", b"title = \"A red kite\"\n"),
            ("netlify.toml", b"[build]\ncommand = \"true\"\n"),
        ],
    );
    let db = load(&dir);
    assert!(
        db.rows
            .get(&grackle_db::Key::new("assets/kite.png.toml"))
            .is_none(),
        "a sidecar declares a row; it is not one"
    );
    assert!(
        !db.by_url.contains_key("/assets/kite.png.toml"),
        "and it has no URL to be fetched at"
    );
    // The control: a `.toml` that speaks for no file is content like anything
    // else, and the site publishes it.
    let out = render(&dir);
    assert!(out.contains_key("/netlify.toml"), "{:?}", out.keys());
    assert!(
        !out.contains_key("/assets/kite.png.toml"),
        "{:?}",
        out.keys()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// **A sidecar'd row is not degenerate**, because it has a name (IO.md §1's
/// softening: the warning nudges an UNNAMED row towards a name).
///
/// This is the one place `renders` and `degenerate` visibly disagree about
/// which question to ask — the law reads the block, the warning reads identity
/// — so the site holds both shapes at once: a sidecar'd `.html` under
/// `shell = "html"` renders WITHOUT a warning, and the blockless `.html` beside
/// it renders WITH one.
///
/// Mutation: pass the block instead of `has_identity` to `shell::degenerate`
/// and the sidecar'd row is warned about while carrying the title the author
/// wrote for it.
#[test]
fn a_sidecard_row_earns_no_degeneracy_warning() {
    let dir = site(
        "degenerate",
        &[
            ("grackle.toml", CONFIG.as_bytes()),
            ("named.html", b"<p>An imported page.</p>\n"),
            (
                "named.html.toml",
                b"title = \"An imported page\"\nshell = \"html\"\n",
            ),
            ("unnamed.html", b"<p>Another one.</p>\n"),
        ],
    );
    let db = load(&dir);
    let named = row(&db, "named.html");
    assert!(named.front_mattered && named.rendered);
    assert_eq!(named.title.as_deref(), Some("An imported page"));
    assert!(
        named.body_bytes > 0,
        "a sidecar'd TEXT file that renders is still all body — the sidecar \
         says what it is, not what is in it"
    );
    // The control, in the same load: the file with no identity at all.
    let unnamed = row(&db, "unnamed.html");
    assert!(!unnamed.front_mattered && !unnamed.rendered);

    assert_eq!(
        db.warnings.len(),
        0,
        "nothing here is degenerate — the sidecar'd row has a name and the \
         other renders `raw`: {:?}",
        db.warnings
    );

    // …and the shape that IS degenerate, so the assertion above is silence
    // that means something rather than a warning path nothing reaches.
    let dir2 = site(
        "degenerate-control",
        &[
            (
                "grackle.toml",
                format!("{CONFIG}\n[[collections]]\nkind = \"tree\"\nsource = \".\"\n\n  \
                         [[collections.rules]]\n  match = \"**/*.html\"\n  route = \"/{{stem}}/\"\n  \
                         defaults = {{ shell = \"html\" }}\n")
                .as_bytes(),
            ),
            ("unnamed.html", b"<p>Another one.</p>\n"),
        ],
    );
    let db2 = load(&dir2);
    assert_eq!(db2.warnings.len(), 1, "{:?}", db2.warnings);
    assert!(
        db2.warnings[0].contains("unnamed.html"),
        "{:?}",
        db2.warnings
    );

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&dir2);
}

/// **A place in the link graph** (IO.md §3): an image field is a foreign key to
/// a row, and a sidecar'd image is that row — carrying the alt text the field's
/// consumer will want.
///
/// The negative half is what makes it a graph rather than a coincidence: point
/// the field at a path no row loads and the load fails naming the field.
#[test]
fn an_image_field_resolves_to_a_sidecard_row() {
    let dir = site(
        "graph",
        &[
            ("grackle.toml", CONFIG.as_bytes()),
            ("assets/kite.png", PNG),
            (
                "assets/kite.png.toml",
                b"title = \"A red kite\"\nalt = \"a red kite over a field\"\n",
            ),
            (
                "_posts/2020-01-01-birds.md",
                b"---\ntitle: Birds\ncover: assets/kite.png\n---\n\nProse.\n",
            ),
        ],
    );
    let db = load(&dir);
    let post = row(&db, "_posts/2020-01-01-birds.md");
    let target = post
        .fields
        .get("cover")
        .and_then(|v| match v {
            Value::Str(s) => Some(s.as_str()),
            _ => None,
        })
        .expect("the post declares a cover");
    let image = row(&db, target);
    assert_eq!(
        image.fields.get("alt"),
        Some(&Value::Str("a red kite over a field".into())),
        "following the edge reaches the sidecar's fields — which is what \
         governance is FOR (q49: the real consumer is alt text)"
    );

    // The edge is checked, and the sidecar does not exempt it.
    let broken = site(
        "graph-broken",
        &[
            ("grackle.toml", CONFIG.as_bytes()),
            ("assets/kite.png", PNG),
            ("assets/kite.png.toml", b"title = \"A red kite\"\n"),
            (
                "_posts/2020-01-01-birds.md",
                b"---\ntitle: Birds\ncover: assets/kit.png\n---\n\nProse.\n",
            ),
        ],
    );
    let e = load_err(&broken);
    assert!(e.contains("cover"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&broken);
}

/// **A row whose identity lives in a second file has to notice that file
/// changing.** `version` is what the incremental machinery compares; without
/// the fold, editing a sidecar changes a row's title and nothing knows.
///
/// Mutation: drop the `^ sc.version` fold and the two versions are equal while
/// the titles differ — a rebuild that skips the row.
#[test]
fn editing_a_sidecar_moves_the_rows_version() {
    let dir = site(
        "version",
        &[
            ("grackle.toml", CONFIG.as_bytes()),
            ("assets/kite.png", PNG),
            ("assets/kite.png.toml", b"title = \"A red kite\"\n"),
        ],
    );
    let before = {
        let db = load(&dir);
        let r = row(&db, "assets/kite.png");
        (r.version, r.title.clone())
    };
    std::fs::write(
        dir.join("assets/kite.png.toml"),
        b"title = \"A red kite over a field\"\n",
    )
    .unwrap();
    let after = {
        let db = load(&dir);
        let r = row(&db, "assets/kite.png");
        (r.version, r.title.clone())
    };
    assert_ne!(before.1, after.1, "the title moved");
    assert_ne!(
        before.0, after.0,
        "so the version must have: the image's own bytes are unchanged, and \
         they are not the whole of what this row is"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// **The description page is deferred, and says so** (IO.md §4a, I11/I12).
///
/// An image with a sidecar CAN wear an html output in the model — that is the
/// object's description page — and it needs an output whose content is not the
/// row's bytes, which the engine does not have yet. So the shape is refused
/// where the author wrote it, for both spellings of identity.
///
/// Mutation: delete the check and the load dies on `stream did not contain
/// valid UTF-8` — the render path reading the PNG as a document body, naming a
/// file and no reason.
#[test]
fn a_picture_may_not_wear_a_document_shell() {
    let config = format!(
        "{CONFIG}\n[[collections]]\nname = \"objects\"\nkind = \"objects\"\n\n  \
         [[collections.rules]]\n  match = \"**/*.png\"\n  route = \"/{{path}}\"\n  \
         defaults = {{ shell = \"html\" }}\n"
    );
    let dir = site(
        "described",
        &[
            ("grackle.toml", config.as_bytes()),
            ("assets/kite.png", PNG),
            ("assets/kite.png.toml", b"title = \"A red kite\"\n"),
        ],
    );
    let e = load_err(&dir);
    assert!(e.contains("assets/kite.png"), "{e}");
    assert!(e.contains("not built yet"), "{e}");
    assert!(e.contains("raw"), "the fix is named: {e}");

    // The same refusal without a sidecar — the law is about the FILE's bytes,
    // not about where identity came from, so a degenerate image is refused too.
    let bare = site(
        "described-bare",
        &[
            ("grackle.toml", config.as_bytes()),
            ("assets/kite.png", PNG),
        ],
    );
    let e = load_err(&bare);
    assert!(e.contains("not built yet"), "{e}");

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&bare);
}

/// **A sidecar is read on the DECLARATION walk**, and this is what that buys:
/// a file-shaped `exclude` pattern is a statement about *content* — grack.com's
/// `exclude` lists `*.toml` — and it must not silently unspeak an identity
/// declaration. MERGE.md R1's narrowing, one declaration family newer.
///
/// Mutation: read sidecars on the content walk instead and this site loses
/// every sidecar it has, silently, with the images still shipping — the
/// failure mode that is invisible in a build's file list.
#[test]
fn a_content_exclude_does_not_unspeak_a_sidecar() {
    let dir = site(
        "excluded",
        &[
            (
                "grackle.toml",
                format!(
                    "{CONFIG}\n[[collections]]\nkind = \"tree\"\nsource = \".\"\n\
                     exclude = [\"*.toml\"]\n"
                )
                .as_bytes(),
            ),
            ("assets/kite.png", PNG),
            ("assets/kite.png.toml", b"title = \"A red kite\"\n"),
        ],
    );
    let db = load(&dir);
    let kite = row(&db, "assets/kite.png");
    assert!(kite.front_mattered && kite.sidecar);
    assert_eq!(kite.title.as_deref(), Some("A red kite"));
    let _ = std::fs::remove_dir_all(&dir);
}

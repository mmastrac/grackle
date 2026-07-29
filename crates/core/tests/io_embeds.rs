//! The embed policy and strong URLs (IO.md §4a, item I11).
//!
//! What this file holds is one sentence with five consequences: **an output
//! has two address slots, and which one a citation takes is decided by the
//! citation's FORM.**
//!
//!  1. a rule may decline to route (`embed = true`), and the policy then gives
//!     its rows a content address under `/static/` — which publishes when
//!     something embeds it and never otherwise;
//!  2. an authored link to such a row is a load error with the fix spelled,
//!     because a hash address is not a link's to make;
//!  3. `[embeds]` off, or narrowed past a row, turns the decline into a
//!     config with no answer — a load error naming the asset;
//!  4. the address is the input bytes plus the transform parameters and
//!     nothing else, so untransformed twins share one address, one store entry
//!     and one output with two inputs;
//!  5. a `/static/` citation resolves back to its input, so the pull publishes
//!     it and the embedding page's `inputs` holds the edge.
//!
//! Built sites throughout: every claim above is about what the finished output
//! contains or about a fact the render pass writes, and the two the load
//! alone can answer say so where they are asserted.

use grackle_core::model::graph::Graph;
use grackle_core::model::{Key, SiteDb};
use grackle_source::strong;
use std::path::{Path, PathBuf};

/// A 2×3 PNG — real bytes, so an image row is an image row.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x08, 0x02, 0x00, 0x00, 0x00, 0x36, 0x88, 0x49,
    0xd6, 0x00, 0x00, 0x00, 0x16, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x60, 0x10, 0x91, 0x03,
    0x21, 0x2e, 0x11, 0x39, 0x2e, 0x10, 0x4b, 0x44, 0x0e, 0x88, 0x00, 0x0d, 0x49, 0x01, 0x69, 0x37,
    0x8b, 0x8f, 0x82, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// The same picture with one byte of comment after `IEND` — a DIFFERENT byte
/// string, and so a different address, while still a decodable PNG. The
/// twin tests need "same bytes" and "different bytes" to be separable facts.
fn png_variant(tag: u8) -> Vec<u8> {
    let mut v = PNG.to_vec();
    v.push(tag);
    v
}

/// The addresses this file predicts, computed the way the engine does.
fn addr(bytes: &[u8]) -> String {
    strong::address(bytes, strong::IDENTITY, "png")
}

/// One site with every address shape beside each other.
///
/// `extends = "none"`, so the two objects rules below are the whole policy
/// this site has: the first ROUTES (`pics/**` — the bookmarkable assets, what
/// grack.com's own config does for its whole corpus), the second DECLINES
/// (`**/*.png` — what the base config ships since I11). The address question
/// is answered once, by the first rule that answers it, which is why a
/// `pics/` image never reaches the second rule.
const SITE: &str = r#"
extends = "none"
[site]
url = "https://example.com"
title = "Embeds"
author = "Tester"

[schema]
shell = { type = "string" }

[[collections]]
name = "objects"

  [[collections.rules]]
  match = "pics/**/*.png"
  route = "/{path}"
  defaults = { shell = "raw" }

  [[collections.rules]]
  match = "**/*.png"
  embed = true
  defaults = { shell = "raw" }

[[collections]]
source = "."

  [[collections.rules]]
  match = "**/*.{md,html}"
  front_matter = true
  route = "/{stem}/"
  defaults = { shell = "html" }

  [[collections.rules]]
  match = "**/*"
  route = "/{path}"
  defaults = { shell = "raw" }

[routes.all]
path = "/all.xml"
shell = "sitemap"
"#;

/// The corpus: one embedded-and-unrouted image, one routed image linked as a
/// download, two unrouted twins holding identical bytes and embedded from two
/// different pages, and one unrouted image nothing mentions at all.
fn files() -> Vec<(String, Vec<u8>)> {
    vec![
        ("grackle.toml".into(), SITE.as_bytes().to_vec()),
        (
            "a.md".into(),
            b"---\ntitle: Ay\n---\n\nAn embed: ![kite](/assets/kite.png)\n\n\
              And a download: [the plate](pics/plate.png)\n"
                .to_vec(),
        ),
        // The raw-HTML twin of `a.md` (§6d stage B): a row whose source IS
        // HTML never meets comrak, so its embeds resolve through the lol_html
        // seam instead — and must resolve identically.
        (
            "d.html".into(),
            b"---\ntitle: Dee\n---\n<p><img src=\"/assets/kite.png\" alt=\"\">\
              <iframe src=\"assets/kite.png\"></iframe></p>\n"
                .to_vec(),
        ),
        (
            "b.md".into(),
            b"---\ntitle: Bee\n---\n\nA twin: ![twin](/assets/twin.png)\n".to_vec(),
        ),
        (
            "c.md".into(),
            b"---\ntitle: Cee\n---\n\nThe other twin: ![twin](/assets/twin2.png)\n".to_vec(),
        ),
        // Only the tree catch-all claims this one, which is what makes the
        // "a rule that declares no address" refusal reachable.
        ("notes.txt".into(), b"Bytes, verbatim.\n".to_vec()),
        ("assets/kite.png".into(), png_variant(1)),
        ("assets/unseen.png".into(), png_variant(2)),
        // Identical bytes, three files, two of them unrouted.
        ("assets/twin.png".into(), PNG.to_vec()),
        ("assets/twin2.png".into(), PNG.to_vec()),
        ("pics/plate.png".into(), PNG.to_vec()),
    ]
}

fn write(dir: &Path, files: &[(String, Vec<u8>)]) {
    let _ = std::fs::remove_dir_all(dir);
    for (rel, body) in files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().expect("a file has a directory")).unwrap();
        std::fs::write(&p, body).unwrap();
    }
}

fn site(who: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("grackle-io-embeds-{who}"));
    write(&dir, &files());
    dir
}

/// The same corpus with the config replaced — for the tests whose subject is a
/// `[embeds]` table or a malformed rule.
fn site_with(who: &str, config: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("grackle-io-embeds-{who}"));
    let mut f = files();
    f[0].1 = config.as_bytes().to_vec();
    write(&dir, &f);
    dir
}

fn load(dir: &Path) -> SiteDb {
    let cfg =
        grackle_core::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    grackle_source::load(&cfg).expect("the site loads")
}

fn load_err(dir: &Path) -> String {
    let cfg = grackle_core::config::Config::load(&dir.join("grackle.toml"));
    match cfg {
        Err(e) => format!("{e:#}"),
        Ok(cfg) => format!(
            "{:#}",
            grackle_source::load(&cfg).expect_err("the site must not load")
        ),
    }
}

fn built(dir: &Path) -> (grackle_core::build::SiteOutput, SiteDb) {
    let cfg =
        grackle_core::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    let mut db = grackle_source::load(&cfg).expect("the site loads");
    let (out, _) = grackle_core::build::render_site(&cfg, &mut db).expect("the site renders");
    let _ = std::fs::remove_dir_all(dir.join("_cache"));
    (out, db)
}

fn build_err(dir: &Path) -> String {
    let cfg =
        grackle_core::config::Config::load(&dir.join("grackle.toml")).expect("the config loads");
    let mut db = grackle_source::load(&cfg).expect("the site loads");
    let e = match grackle_core::build::render_site(&cfg, &mut db) {
        Ok(_) => panic!("the render must fail"),
        Err(e) => format!("{e:#}"),
    };
    let _ = std::fs::remove_dir_all(dir.join("_cache"));
    e
}

fn row<'a>(db: &'a SiteDb, rel: &str) -> &'a grackle_core::model::Row {
    db.rows
        .iter()
        .find(|r| r.rel.to_string_lossy() == rel)
        .unwrap_or_else(|| panic!("{rel} is a row"))
}

fn text(out: &grackle_core::build::SiteOutput, url: &str) -> String {
    String::from_utf8(
        out.get(url)
            .unwrap_or_else(|| panic!("{url} was published"))
            .clone(),
    )
    .expect("a document is text")
}

// ---------------------------------------------------------------------------

/// **The policy is the default, and the pull is still the garbage collector.**
///
/// A rule declined to route `assets/kite.png`, so the row has no canonical URL
/// and one strong one; the page that embeds it resolves to that address, and
/// the bytes land there. `assets/unseen.png` was declined by the same rule and
/// nothing embeds it, so it publishes nowhere — which is §4a's sentence about
/// an image nothing cites, now true of an image no rule routes either.
///
/// The three facts are asserted together on purpose: an address with no bytes
/// under it is a 404 with extra steps, and bytes at an address nothing points
/// to are a file nobody can reach.
///
/// Mutations, each red and each restored:
///
/// - delete the `strong_url` arm from `materialize_referenced`'s `at` match →
///   the address resolves and the bytes never publish (the 404);
/// - make `links::resolve`'s embed branch return `Ok(None)` always → the page
///   ships `src="/assets/kite.png"`, which no route answers;
/// - drop `strong_url` from `by_strong` (index nothing) → both of the above at
///   once, since the citation resolves to no input at all.
#[test]
fn the_policy_addresses_an_embedded_asset_no_rule_routed() {
    let dir = site("default");
    let (out, db) = built(&dir);

    let kite = addr(&png_variant(1));
    assert_eq!(
        out.get(&kite).map(Vec::as_slice),
        Some(png_variant(1).as_slice()),
        "the bytes land at their content address"
    );
    assert!(
        text(&out, "/a/").contains(&format!("src=\"{kite}\"")),
        "and the embed points at it: {}",
        text(&out, "/a/")
    );
    // §6d stage B: the same embed in a raw-HTML row resolves the same way,
    // and `<iframe>` is in the embed set beside `<img>`. Both spellings of the
    // path — root-relative and relative to the embedding file — reach it.
    let d = text(&out, "/d/");
    assert_eq!(
        d.matches(&format!("src=\"{kite}\"")).count(),
        2,
        "the raw-HTML seam resolves img and iframe alike: {d}"
    );
    assert!(
        !out.contains_key(&addr(&png_variant(2))),
        "an asset no rule routes and nothing embeds ships nowhere — the pull \
         is the garbage collector (IO.md §4a)"
    );

    // The row side of the same three facts.
    let r = row(&db, "assets/kite.png");
    assert_eq!(r.url, "", "an embed-addressed row has no canonical address");
    assert_eq!(r.strong_url.as_deref(), Some(kite.as_str()));
    assert!(
        r.output.is_some(),
        "and it landed, because something embedded it"
    );
    assert!(
        row(&db, "assets/unseen.png").output.is_none(),
        "while its uncited neighbour did not"
    );
}

/// **The address is computable at planning** — IO.md §4a's hashing law, which
/// is what keeps §1's "facts at planning; content at materialization" true of
/// an output whose whole address is a hash.
///
/// Asserted on a LOADED site, before any shell has run and before a single
/// output byte exists: the address is already on the row, and it is exactly
/// what `strong::address` says the INPUT bytes and the identity transform's
/// parameters produce. An address derived from output bytes could not exist at
/// this point, which is the whole argument in one assertion.
///
/// The extension travels with the URL and nothing else about the path does:
/// two files at two paths holding one byte string get one address (the twin
/// test below builds on this), so the address cannot be a function of the
/// name.
///
/// Mutation: hash `f.rel` beside the bytes in `load::embed_address` — the
/// twins below stop sharing an address and this equality goes red, which is
/// the same defect seen from two sides.
#[test]
fn the_address_is_the_inputs_and_the_parameters() {
    let db = load(&site("law"));
    assert_eq!(
        row(&db, "assets/kite.png").strong_url.as_deref(),
        Some(addr(&png_variant(1)).as_str()),
        "the address exists at load, and it is blake3(input bytes + variant)"
    );
    assert_eq!(
        row(&db, "assets/twin.png").strong_url,
        row(&db, "assets/twin2.png").strong_url,
        "two paths, one byte string, one address"
    );
    assert_ne!(
        row(&db, "assets/kite.png").strong_url,
        row(&db, "assets/twin.png").strong_url,
        "and different bytes are simply a different key"
    );
}

/// **The untransformed-twin rule** (IO.md §4a's third bullet), live.
///
/// Three files hold one byte string. One is ROUTED, so its citations take its
/// canonical address and its bytes ship there. The other two are
/// embed-addressed, and because the address is a pure function of the bytes
/// they land at ONE `/static/` entry — which is also, by construction, what
/// the routed one's strong URL would be if anything asked for it. That is the
/// design's sentence "an untransformed embed of a routed output shares bytes,
/// so its hash address IS that output's strong URL", and it is arithmetic
/// rather than machinery: nothing in the engine compares the two.
///
/// The dedupe has a shape in the database too, and it is the interesting one:
/// **one output, two inputs.** The second twin does not mint a second route
/// over the first; it joins the existing one's `inputs`, which is what makes
/// the fanout of EITHER file reach the page that embeds the OTHER — correct,
/// because editing either one changes which bytes that address stands for.
///
/// Mutation: delete the `db.routes.get_mut(&at_key)` twin branch in
/// `materialize_referenced` → the second twin pushes a duplicate route at one
/// URL, and the `inputs` assertion goes red with it.
#[test]
fn untransformed_twins_share_one_address_and_one_entry() {
    let dir = site("twins");
    let (out, db) = built(&dir);

    let shared = addr(PNG);
    assert_eq!(out.get(&shared).map(Vec::as_slice), Some(PNG), "one entry");
    assert_eq!(
        out.keys().filter(|u| u.as_str() == shared).count(),
        1,
        "and exactly one"
    );
    for (page, who) in [("/b/", "twin"), ("/c/", "twin2")] {
        assert!(
            text(&out, page).contains(&format!("src=\"{shared}\"")),
            "{who}'s page embeds the shared address: {}",
            text(&out, page)
        );
    }

    // The routed member of the trio is untouched: its bytes ship at the
    // address its rule declared, and the authored link resolves there.
    assert_eq!(out.get("/pics/plate.png").map(Vec::as_slice), Some(PNG));
    assert!(
        text(&out, "/a/").contains("href=\"/pics/plate.png\""),
        "an authored link takes the canonical address: {}",
        text(&out, "/a/")
    );

    // One output, two inputs — the dedupe as the graph sees it.
    let o = db
        .routes
        .get(&Key::new(&shared))
        .expect("the strong output is a route like any other");
    let inputs: Vec<String> = o.inputs.iter().map(|k| k.to_string()).collect();
    assert!(
        inputs.iter().any(|i| i == "assets/twin.png")
            && inputs.iter().any(|i| i == "assets/twin2.png"),
        "both twins are inputs to the one artifact: {inputs:?}"
    );
    assert_eq!(
        o.strong_url.as_deref(),
        Some(shared.as_str()),
        "and the output carries the slot, equal to its url because no rule \
         gave it another address"
    );
}

/// **A strong citation is an `inputs` edge** — review I-D's design catch, and
/// the reason this item had to touch citation resolution at all.
///
/// `db.by_url` holds canonical row URLs only, so a `/static/{hash}` citation
/// resolves to NOTHING there: the embedding page's `inputs` would silently
/// lose the asset, the graph would lose the edge, and an incremental rebuild
/// would ship `/a/` unchanged after the picture in it was replaced. The second
/// index is what closes it, and the fanout is where the closure is visible —
/// editing the asset must reach the page.
///
/// Mutation: make `build::resolve_citation` consult `by_url` alone → the edge
/// vanishes from `inputs`, `/a/` leaves the asset's fanout, and
/// `io_graph.rs`'s consistency guard is the shape that would have caught it on
/// a routed asset.
#[test]
fn a_strong_citation_produces_the_inputs_edge() {
    let dir = site("edge");
    let (_, db) = built(&dir);

    let page = db
        .routes
        .get(&Key::new("/a/"))
        .expect("the embedding page is an output");
    let inputs: Vec<String> = page.inputs.iter().map(|k| k.to_string()).collect();
    assert!(
        inputs.iter().any(|i| i == "assets/kite.png"),
        "the embedded asset is an input to the page's bytes: {inputs:?}"
    );

    let fan: Vec<String> = Graph::of(&db)
        .fanout(&grackle_core::model::graph::Node::Input(Key::new(
            "assets/kite.png",
        )))
        .iter()
        .map(|n| n.key().to_string())
        .collect();
    assert!(
        fan.iter().any(|u| u == "/a/"),
        "so editing the asset reaches the page that shows it: {fan:?}"
    );
}

/// **An authored link demands a route** (IO.md §4a's second bullet), with the
/// fix spelled where the author is standing.
///
/// The target is a known row — the resolver found it — and the config declined
/// to give it a canonical address. Answering with the strong one would be
/// worse than failing: a content address changes the day the bytes do, and a
/// link is a promise that it will not.
///
/// The control is in the same fixture and is the half that could rot silently:
/// a link to a ROUTED asset still resolves, so this refusal is about the
/// address the config declared and not about linking assets.
///
/// Mutations, each red and each restored:
///
/// - delete the `source_to_strong` branch from `links::resolve`'s candidate
///   loop → the link falls through to strict mode's generic "matches no source
///   file or route", which names the file and not the decision;
/// - answer it with the strong address instead of failing → the site builds
///   and ships a hash URL in prose, which is the outcome the refusal exists to
///   prevent.
#[test]
fn an_authored_link_to_an_unrouted_asset_is_refused() {
    let dir = site("link");
    std::fs::write(
        dir.join("a.md"),
        b"---\ntitle: Ay\n---\n\nA link: [the kite](/assets/kite.png)\n\n\
          And a good one: [the plate](pics/plate.png)\n",
    )
    .unwrap();
    let e = build_err(&dir);
    assert!(e.contains("assets/kite.png"), "it names the asset: {e}");
    assert!(e.contains("no rule routes"), "and the diagnosis: {e}");
    assert!(
        e.contains("route = \"/{path}\"") && e.contains("embed it instead"),
        "and both fixes: {e}"
    );

    // The control: the routed asset's link is fine, and was in the same file.
    let dir = site("link-control");
    std::fs::write(
        dir.join("a.md"),
        b"---\ntitle: Ay\n---\n\nA good one: [the plate](pics/plate.png)\n",
    )
    .unwrap();
    let (out, _) = built(&dir);
    assert!(text(&out, "/a/").contains("href=\"/pics/plate.png\""));
}

/// **`[embeds]` off, and `[embeds]` narrowed** — the policy's two config
/// answers, both landing as a load error that names the asset.
///
/// With the policy off, `embed = true` is a rule that names no address at all:
/// the row can be reached by nothing, which is the config forgetting rather
/// than the config deciding. Asked at LOAD rather than at the citation, which
/// is stricter than §4a's letter and deliberately so — the question is about
/// the config's shape, so it needs no citation to be answerable, and the
/// message can name a path instead of a URL that does not exist.
///
/// The subset case is the same error one clause over, and it carries the globs
/// so a reader can see what would have to widen.
///
/// The control is the third assertion: a subset that ADMITS the asset loads,
/// so the refusal is about the filter and not about the key being present.
///
/// Mutations, each red and each restored: delete the `enabled` guard (the site
/// loads and publishes a hash address the config asked it not to); delete the
/// subset guard (same, one clause over); make the subset default to matching
/// nothing rather than everything (every site with an embed rule dies).
#[test]
fn the_policy_off_or_narrowed_is_a_load_error_naming_the_asset() {
    let off = SITE.replace("[routes.all]", "[embeds]\nenabled = false\n\n[routes.all]");
    let e = load_err(&site_with("off", &off));
    assert!(e.contains("assets/kite.png"), "it names the asset: {e}");
    assert!(e.contains("enabled = false"), "and the key: {e}");
    assert!(e.contains("reached by nothing"), "and the consequence: {e}");

    let narrow = SITE.replace(
        "[routes.all]",
        "[embeds]\nmatch = \"gallery/**\"\n\n[routes.all]",
    );
    let e = load_err(&site_with("narrow", &narrow));
    assert!(e.contains("assets/kite.png"), "it names the asset: {e}");
    assert!(
        e.contains("does not admit it") && e.contains("gallery/**"),
        "and the filter that would have to widen: {e}"
    );

    // The control: a subset that admits it is not a refusal.
    let wide = SITE.replace(
        "[routes.all]",
        "[embeds]\nmatch = [\"assets/**\", \"gallery/**\"]\n\n[routes.all]",
    );
    let (out, _) = built(&site_with("wide", &wide));
    assert!(out.contains_key(&addr(&png_variant(1))));
}

/// **A rule decides an address once** — the config-time refusal, in both
/// directions.
///
/// `route` and `embed` are two answers to one question and they are not
/// layers: a routed output wins, so a fallback beneath it can never be
/// reached, and a reader cannot tell which of the two lines is the mistake.
/// `on_demand` beside `embed` is the I7b dead-key family — it defers a route
/// this rule does not mint.
///
/// Both are asked in `Config::from_toml`, so no walk and no file can change
/// the answer and every declared profile is projected through the same
/// deserializer.
///
/// Mutations, each red and each restored: delete either bail (the config loads
/// and the rule's second key configures nothing); narrow the check to
/// non-inherited rules (both halves still pass, and a base that grew the
/// mistake would ship it — the IR9 argument, one key over).
#[test]
fn a_rule_may_not_declare_two_addresses() {
    let both = SITE.replace(
        "  match = \"**/*.png\"\n  embed = true",
        "  match = \"**/*.png\"\n  route = \"/x/{path}\"\n  embed = true",
    );
    let e = load_err(&site_with("both", &both));
    assert!(e.contains("both `route`"), "the diagnosis: {e}");
    assert!(e.contains("could never be reached"), "and the reason: {e}");

    let defer = SITE.replace(
        "  match = \"**/*.png\"\n  embed = true",
        "  match = \"**/*.png\"\n  embed = true\n  on_demand = true",
    );
    let e = load_err(&site_with("defer", &defer));
    assert!(e.contains("configures nothing"), "the dead key: {e}");
}

/// **The `{hash}` route token** (IO.md §4a's fourth bullet): a site that wants
/// hashed CANONICAL addresses says so in a route template, and gets the same
/// string the policy would have minted.
///
/// That equality is the point rather than a coincidence — one hash function,
/// one address per byte string, whichever mechanism asked — and it is what
/// makes the two mechanisms describable as two spellings of one idea instead
/// of two schemes that happen to rhyme. Since the rule ROUTES, the row's
/// address is canonical: an authored link to it resolves, which is the
/// difference a site buys by spelling it this way.
///
/// The rule is narrowed to one file on purpose, and the reason is worth
/// recording: routing the TWINS by `{hash}` is a **route collision**, refused
/// by the pre-existing unique-URL check naming both files. That is right — a
/// canonical address belongs to one row, which is §4's law and not this item's
/// to bend — and it is the sharpest statement of how the two mechanisms
/// differ. The policy's address is a place in the content store, so sharing it
/// is dedupe; a route is a row's own address, so sharing it is a collision.
///
/// Mutation: make the token hash something else (the path, a constant) → the
/// equality goes red while the site still builds, which is exactly the drift
/// this pins.
#[test]
fn a_route_may_spend_the_content_hash() {
    let hashed = SITE.replace(
        "  match = \"**/*.png\"\n  embed = true",
        "  match = \"assets/kite.png\"\n  route = \"/img/{hash}.{ext}\"\n\n\
         \x20 [[collections.rules]]\n  match = \"**/*.png\"\n  embed = true",
    );
    let dir = site_with("hash", &hashed);
    let db = load(&dir);
    let r = row(&db, "assets/kite.png");
    let want = addr(&png_variant(1));
    let digest = want.rsplit('/').next().expect("an address has a basename");
    assert_eq!(
        r.url,
        format!("/img/{digest}"),
        "the token spends the same digest the policy would have"
    );
    assert!(
        r.strong_url.is_none(),
        "and this row is ROUTED — the canonical slot is filled, not the strong one"
    );

    // Routed, so an authored link to it resolves rather than being refused.
    std::fs::write(
        dir.join("a.md"),
        b"---\ntitle: Ay\n---\n\nA link: [the kite](/assets/kite.png)\n",
    )
    .unwrap();
    let (out, _) = built(&dir);
    assert!(
        text(&out, "/a/").contains(&format!("href=\"/img/{digest}\"")),
        "{}",
        text(&out, "/a/")
    );
}

/// **A rule that says neither is still the error it always was** — the guard
/// this item had to be careful not to delete.
///
/// "No rule supplies a route" is what a claimed row with no address has meant
/// since there was a walk, and the whole reason `embed = true` is a DECLARED
/// key rather than the absence of `route` is to keep it: a rule that says
/// neither has forgotten, a rule that says one has decided, and the engine can
/// tell them apart because the author wrote which.
///
/// Mutation: read "no route template" as "embed-addressed" in `walk_site` —
/// this site loads, the `.md` beside it silently stops publishing, and the
/// only refusal that catches a rules-gap goes with it.
#[test]
fn a_rule_that_declares_no_address_at_all_still_fails() {
    let silent = SITE.replace(
        "  match = \"**/*\"\n  route = \"/{path}\"\n  defaults = { shell = \"raw\" }",
        "  match = \"**/*\"\n  defaults = { shell = \"raw\" }",
    );
    let e = load_err(&site_with("silent", &silent));
    assert!(
        e.contains("no rule supplies a route"),
        "the pre-existing refusal, untouched: {e}"
    );
}

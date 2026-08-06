//! The join fields.
//!
//! Two databases, joined on three explicit fields: `output` and `viewed_by` on
//! an input row, `inputs` on an output. The tests below are organised around
//! the two questions the ledger asks of a new column, *what does it say for
//! every shape of row the phase created*, and *where is it built*, because
//! the second is what decides whether the first is ever true when someone
//! reads it.
//!
//! Built sites where the claim needs bytes (the citation closure, the pull
//! model's on-demand row), loaded sites everywhere else. `render_site` takes
//! `&mut SiteDb`, so a test can ask the database what the render pass wrote
//! into it, which is the only way to see a fact that is decided after every
//! filter has run.

use grackle_core::filter::{Filter, Row as _, Value};
use grackle_core::model::SiteDb;
use std::path::PathBuf;

mod support;
use support::{built, load};

/// A 2×3 PNG, real bytes, so an image row is an image row.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x08, 0x02, 0x00, 0x00, 0x00, 0x36, 0x88, 0x49,
    0xd6, 0x00, 0x00, 0x00, 0x16, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x60, 0x10, 0x91, 0x03,
    0x21, 0x2e, 0x11, 0x39, 0x2e, 0x10, 0x4b, 0x44, 0x0e, 0x88, 0x00, 0x0d, 0x49, 0x01, 0x69, 0x37,
    0x8b, 0x8f, 0x82, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

fn site(who: &str, files: &[(&str, &[u8])]) -> PathBuf {
    support::site("io-join", who, files)
}

/// A row by source path, identity, so a row with no URL is reachable.
fn row<'a>(db: &'a SiteDb, rel: &str) -> &'a grackle_core::model::Row {
    db.rows
        .get(&grackle_core::model::Key::new(rel))
        .unwrap_or_else(|| panic!("no row at {rel}"))
}

/// `output.url` as a plain string, or `None` for a row that lands nowhere.
fn output_url(r: &grackle_core::model::Row) -> Option<String> {
    match r.field("output.url") {
        Value::Str(s) => Some(s),
        Value::Null => None,
        other => panic!("output.url answered {other:?}"),
    }
}

fn lands(r: &grackle_core::model::Row) -> bool {
    r.field("output") == Value::Bool(true)
}

fn keys(ks: &[grackle_core::model::Key]) -> Vec<String> {
    ks.iter().map(|k| k.to_string()).collect()
}

fn route<'a>(db: &'a SiteDb, url: &str) -> &'a grackle_core::model::Route {
    db.routes
        .get(&grackle_core::model::Key::new(url))
        .unwrap_or_else(|| panic!("no route at {url}"))
}

// ---------------------------------------------------------------------------

const SHAPES: &str = r#"
extends = "none"
[site]
url = "https://example.com"
title = "Shapes"
author = "Tester"

[schema]
shell = { type = "string" }
alt = { type = "string" }

[[collections]]
name = "objects"

  # on-demand: the URL is computed, the route is not minted until something
  # references the file. grack.com's own images rule, minimised.
  [[collections.rules]]
  match = "pics/**/*.png"
  route = "/{path}"
  on_demand = true
  defaults = { shell = "raw" }

[[collections]]
source = "."

  # A blockless `.md` sent through a document shell is a DEGENERATE row:
  # it renders, it lands, and it has no identity.
  [[collections.rules]]
  match = "notes/*.md"
  route = "/notes/{stem}/"
  defaults = { shell = "html" }

  [[collections.rules]]
  match = "**/*.md"
  front_matter = true
  route = "/{stem}/"
  defaults = { shell = "html" }

  [[collections.rules]]
  match = "**/*"
  route = "/{path}"
  defaults = { shell = "raw" }

# the landing owns the URL, and `landing.md` loses its own route — the
# `!output` shape the join makes sayable.
[routes.things]
path = "/things/"
from = "entries"
where = 'glob(path, "notes/*.md")'
layout = "card"
content = "landing.md"
title = "Things"
"#;

/// The shapes site, per test: two tests read it and each removes its tree at
/// both ends, which is a race when they share one directory and run in
/// parallel (the same fixture-race note applies one file over).
fn shapes_site(who: &str) -> PathBuf {
    site(
        who,
        &[
            ("grackle.toml", SHAPES.as_bytes()),
            // Degenerate: no block, a document shell.
            ("notes/plain-note.md", b"Just words, no block.\n"),
            // The landing body a view claims.
            (
                "landing.md",
                b"---\ntitle: Things\n---\nThe landing's own words.\n\n{% view things %}\n",
            ),
            // A page that cites one image and not the other.
            (
                "gallery.md",
                b"---\ntitle: Gallery\n---\n![kite](/pics/kite.png)\n\nSee [elsewhere](https://example.org/away).\n",
            ),
            ("pics/kite.png", PNG),
            ("pics/unseen.png", PNG),
            // Identity from a sidecar: a governed row whose bytes are never
            // parsed.
            ("assets/badge.png", PNG),
            (
                "assets/badge.png.toml",
                b"title = \"Badge\"\nalt = \"a badge\"\n",
            ),
        ],
    )
}

/// **The five row shapes the phase created or sharpened, each asked `output`.**
///
/// One site, because the answers are only interesting beside each other: the
/// column has to be right for a row with no identity, a row with identity and
/// no parsed content, a row whose URL belongs to something else, and a row
/// whose URL exists but whose route does not. Four different reasons, one
/// question.
///
/// The **on-demand** row is the one that needs the render pass, and it is the
/// pull model stated as a test: `output` is `None` for the whole of the load,
/// and `Some` after `materialize_referenced`, but only for the image
/// something cited. The uncited twin stays `None` forever, which is
/// "the pull model is the garbage collector" from the join's side.
///
/// Mutations, each red: delete `join_outputs`' call after route minting (every
/// row lands nowhere); delete the `row.output` assignment in
/// `materialize_referenced` (the cited image never lands, while the site still
/// publishes it, the half a build's file list cannot see); make `output.url`
/// answer `Str("")` instead of Null (the claimed row reads as landing at the
/// empty URL, and `lands` and `output_url` stop agreeing).
#[test]
fn every_row_shape_answers_output() {
    let dir = shapes_site("shapes");
    let db = load(&dir);

    // 1. Degenerate: no identity, and it lands all the same. Identity is not
    //  what mints a URL, the rule is.
    let degenerate = row(&db, "notes/plain-note.md");
    assert!(!degenerate.front_mattered, "no block, so no identity");
    assert!(lands(degenerate));
    assert_eq!(
        output_url(degenerate).as_deref(),
        Some("/notes/plain-note/")
    );

    // 2. Sidecar'd: identity without content, and its output is its bytes.
    let badge = row(&db, "assets/badge.png");
    assert!(badge.front_mattered && badge.sidecar && !badge.rendered);
    assert_eq!(output_url(badge).as_deref(), Some("/assets/badge.png"));

    // 3. Claimed: the landing owns the URL, so the row lands nowhere. The
    //  structural exclusion, as a fact anything can read.
    let landing = row(&db, "landing.md");
    assert!(landing.claimed);
    assert!(!lands(landing), "a claimed row has no output of its own");
    assert_eq!(output_url(landing), None, "and no address to report");

    // 4. On-demand, unreferenced, at load, BOTH images. The URL is computed
    //  (that is what lets a citation resolve), the output is not.
    let kite = row(&db, "pics/kite.png");
    assert_eq!(kite.url, "/pics/kite.png", "the URL is computed at load");
    assert!(!lands(kite), "…and nothing has referenced it yet");
    assert!(!lands(row(&db, "pics/unseen.png")));

    // 5. …and after the render pass, exactly the cited one.
    let (out, db) = built(&dir);
    assert!(
        out.contains_key("/pics/kite.png"),
        "the citation published it"
    );
    assert!(!out.contains_key("/pics/unseen.png"));
    let kite = row(&db, "pics/kite.png");
    assert!(
        lands(kite),
        "bare `output` is truthy once something referenced it"
    );
    assert_eq!(output_url(kite).as_deref(), Some("/pics/kite.png"));
    assert!(
        !lands(row(&db, "pics/unseen.png")),
        "and the uncited twin still lands nowhere — the pull model, from the \
         join's side"
    );

    // **The two dashes that have a name say it.** `explain` prints `url` two
    // lines above `output`, and for a claimed row those are the landing's URL
    // and no output at all, which reads as a contradiction to anyone who does
    // not already know the rule. Mutations: hardcode either reason (the other row
    // fails), or drop both (the claimed row and the unreferenced image become
    // indistinguishable from a row no rule routed).
    assert!(
        grackle_core::debug::row_facts(row(&db, "landing.md"))
            .contains("output      - (claimed — the landing at that URL owns it)"),
        "the claimed row says why it has none"
    );
    assert!(
        grackle_core::debug::row_facts(row(&db, "pics/unseen.png"))
            .contains("output      - (on demand — nothing has referenced it)"),
        "and so does the one nothing has asked for"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// **`!output` selects exactly the rows a landing claimed**.
///
/// Both claim shapes in one site, because they are settled at different times
/// and only one of them needs correcting: a LITERAL claim withholds the route
/// before it is ever minted, while a TEMPLATED one mints routes and retracts
/// them once the group keys exist. A `join_outputs` called only at minting
/// gets the literal case right and the templated case wrong, which is the
/// second mutation below.
///
/// The site deliberately has no on-demand rows, so `!output` is the claimed
/// set exactly rather than approximately, the other way a row lands nowhere
/// is the pull model, and mixing the two would make the assertion vacuous.
///
/// Mutations, each red: delete either `join_outputs` call (the first empties
/// the selection's complement, the second leaves `beta/index.md` reporting the
/// route it lost); the control is `every_other_row_lands`, which is what says
/// this is a filter and not a constant.
#[test]
fn not_output_is_the_landings_exclusion() {
    let dir = site(
        "landings",
        &[
            (
                "grackle.toml",
                br#"
extends = "none"
[site]
url = "https://example.com"
title = "Landings"
author = "Tester"

[schema]
shell = { type = "string" }
topic = { type = "string" }
date = { type = "date" }

[[collections]]
name = "posts"
source = "_posts"
file = ["{date.year}-{date.month}-{date.day}-{slug}"]

  [[collections.rules]]
  match = "**/*.{md,markdown}"
  route = "/blog/{slug}/"
  defaults = { shell = "html" }

[[collections]]
source = "."

  [[collections.rules]]
  match = "*/index.md"
  front_matter = true
  route = "/{dir}/"
  defaults = { shell = "html" }

  [[collections.rules]]
  match = "**/*.md"
  front_matter = true
  route = "/{stem}/"
  defaults = { shell = "html" }

  [[collections.rules]]
  match = "**/*"
  route = "/{path}"
  defaults = { shell = "raw" }

# A TEMPLATED claim: one route per group, each embedding its own body, and the
# claim is only settled once the group keys exist.
[routes.topics]
path = "/{group:key}/"
from = "posts"
group_by = "topic"
layout = "card"
content = "{group:key}/index.md"
title = "{group:key}"

# A LITERAL claim: settled at load, the route never minted.
[routes.everything]
path = "/everything/"
from = "posts"
layout = "card"
content = "everything.md"
title = "Everything"

# The column read by a VIEW, which is the whole reason `output` is built at
# route-minting time rather than with the other two join fields: a filter that
# runs before the fact exists selects nothing, forever, silently.
[routes.landed]
path = "/landed/"
from = "entries"
where = "output"
layout = "card"
title = "Landed"
"#,
            ),
            (
                "_posts/2020-01-01-first.md",
                b"---\ntitle: First\ntopic: alpha\n---\nOne.\n",
            ),
            (
                "_posts/2020-01-02-second.md",
                b"---\ntitle: Second\ntopic: beta\n---\nTwo.\n",
            ),
            ("alpha/index.md", b"---\ntitle: Alpha\n---\nAlpha words.\n"),
            ("beta/index.md", b"---\ntitle: Beta\n---\nBeta words.\n"),
            (
                "everything.md",
                b"---\ntitle: Everything\n---\nAll of it.\n",
            ),
            ("loose.md", b"---\ntitle: Loose\n---\nUnclaimed.\n"),
        ],
    );
    let db = load(&dir);

    let landless =
        Filter::parse("!output", &grackle_core::model::row_schema()).expect("`!output` parses");
    let mut selected: Vec<String> = db
        .rows
        .matching(&landless)
        .map(|r| r.rel.to_string_lossy().to_string())
        .collect();
    selected.sort();
    assert_eq!(
        selected,
        ["alpha/index.md", "beta/index.md", "everything.md"],
        "the claimed rows and nothing else — two settled by the templated \
         claim after the routes existed, one by the literal claim before"
    );
    // The claim is the reason, not an accident of the query.
    for rel in &selected {
        assert!(row(&db, rel).claimed, "{rel} is claimed");
    }

    // **A view reads it, and that is what dates the field.** `build_views`
    // runs before templated claim is settled, so `output` has to exist
    // before it, a `where` evaluated against an unbuilt column selects
    // nothing and says nothing, which is the silent-empty knife this ledger
    // keeps taking away. Mutation: delete `join_outputs`' call after route
    // minting and this listing is empty while the site still builds.
    let landed = keys(&route(&db, "/landed/").inputs);
    assert!(
        landed.contains(&"loose.md".to_string()),
        "the listing selected on `output` and found the rows that land: {landed:?}"
    );
    assert!(
        !landed.contains(&"everything.md".to_string()),
        "and not the claimed ones"
    );

    // The control: `!output` is a predicate, so something has to fail it.
    let lands = Filter::parse("output", &grackle_core::model::row_schema()).unwrap();
    let landing_rows: Vec<String> = db
        .rows
        .matching(&lands)
        .map(|r| r.rel.to_string_lossy().to_string())
        .collect();
    assert!(
        landing_rows.contains(&"loose.md".to_string())
            && landing_rows.contains(&"_posts/2020-01-01-first.md".to_string()),
        "every other row lands: {landing_rows:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// **Arrangement is not citation**, which is the distinction the
/// backlink scanner learned the hard way and this is the field that names it.
///
/// One row is carried by a listing; a second row merely links to it. `viewed_by`
/// fires for the first and not the second, and the citation is still visible
/// where citations live (the link graph), so the test is about which field
/// answers rather than about whether anything did.
///
/// `inputs` is the same edge read backwards, and asserted here for that reason:
/// the listing's `inputs` is exactly its members, so the two fields are one
/// relation and a mutation that breaks one breaks both.
///
/// Mutations, each red: delete the `join_arrangement` call (`viewed_by` and
/// `inputs` both empty); fill `viewed_by` from `route_members` instead of
/// `members` (the listing stops naming the row it lists); push the citing
/// page's route into `viewed_by` (the arrangement/citation split collapses,
/// and the second assertion is the only one that sees it).
#[test]
fn viewed_by_is_arrangement_and_not_citation() {
    let dir = site(
        "arrangement",
        &[
            (
                "grackle.toml",
                br#"
extends = "none"
[site]
url = "https://example.com"
title = "Arrangement"
author = "Tester"

[schema]
shell = { type = "string" }
date = { type = "date" }

[[collections]]
name = "posts"
source = "_posts"
file = ["{date.year}-{date.month}-{date.day}-{slug}"]

  [[collections.rules]]
  match = "**/*.{md,markdown}"
  route = "/blog/{slug}/"
  defaults = { shell = "html" }

[[collections]]
source = "."

  [[collections.rules]]
  match = "**/*.md"
  front_matter = true
  route = "/{stem}/"
  defaults = { shell = "html" }

  [[collections.rules]]
  match = "**/*"
  route = "/{path}"
  defaults = { shell = "raw" }

[routes.blog]
path = "/blog/"
from = "posts"
layout = "card"
title = "Blog"
"#,
            ),
            (
                "_posts/2020-01-01-listed.md",
                b"---\ntitle: Listed\n---\nA post a listing carries.\n",
            ),
            (
                "citing.md",
                b"---\ntitle: Citing\n---\nSee [it](/blog/listed/).\n",
            ),
        ],
    );
    let db = load(&dir);

    let listed = row(&db, "_posts/2020-01-01-listed.md");
    assert_eq!(
        keys(&listed.viewed_by),
        ["/blog/"],
        "the listing that carries it as a member — and ONLY that: the page \
         citing it is `linked_from`'s business, not this field's"
    );
    assert!(
        !keys(&listed.viewed_by).contains(&"/citing/".to_string()),
        "a citation is not an arrangement"
    );

    // The same edge from the output side.
    assert_eq!(
        keys(&route(&db, "/blog/").inputs),
        ["_posts/2020-01-01-listed.md"],
        "a listing's inputs are its members"
    );
    // The citing page is arranged by nothing and carries only itself.
    assert!(row(&db, "citing.md").viewed_by.is_empty());
    assert_eq!(keys(&route(&db, "/citing/").inputs), ["citing.md"]);

    let _ = std::fs::remove_dir_all(&dir);
}

/// **`inputs` is the full row-level closure**, and its two halves are built at two different times for one reason:
/// a citation is a fact about *content*.
///
/// The gallery page's inputs are its own row (planning) plus the image it
/// cites (render), and the image is on-demand, so this is also the edge that
/// explains why it got published at all. Non-row dependencies are absent by
/// construction rather than by a filter: a `.slots/` fill or a theme file is
/// not a row, so "row-level closure" already excludes them and the typed
/// invalidation keys keep them.
///
/// The scanner is the UNFENCED one (`cited_urls`, the one on-demand publishing
/// reads) rather than the backlink graph's `cited_urls_cited`, and that is a
/// decision rather than a guard: splice fence exists to keep a listing's
/// arrangement out of "linked from", which is a presentation question, while
/// an image a listing arranged is still an input to the bytes. Measured, the
/// two agree on every shape this suite and the corpus can build, a splice
/// cites its members (already inputs by membership) and thumbnail URLs (not
/// rows), so the choice is recorded here rather than pinned by a mutation
/// that no fixture can make red.
///
/// Mutations, each red: delete the `join_citations` call (the image leaves the
/// gallery's inputs while the site still publishes it); resolve a cited URL to
/// a key whether or not `by_url` knows it (the external link becomes an edge,
/// which the exact-equality assertion below is the control for).
#[test]
fn inputs_closes_over_citations() {
    let dir = shapes_site("closure");
    let (_, db) = built(&dir);

    assert_eq!(
        keys(&route(&db, "/gallery/").inputs),
        ["gallery.md", "pics/kite.png"],
        "the row it renders, and the rows its finished bytes cite"
    );
    assert!(
        !keys(&route(&db, "/gallery/").inputs).contains(&"pics/unseen.png".to_string()),
        "an image nothing cites feeds nothing"
    );

    // The landing: its members, plus the row it claims as its body, the one
    // input a listing has that its member list does not name.
    let things = keys(&route(&db, "/things/").inputs);
    assert!(
        things.contains(&"landing.md".to_string()),
        "the claimed content row is an input: {things:?}"
    );
    assert!(
        things.contains(&"notes/plain-note.md".to_string()),
        "so are the members: {things:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------

/// **An axis row's output is the CANONICAL member's URL, and the rest are
/// `alternates`**.
///
/// The axis fixture's shape, minimised: one row published under two themes and
/// one row published at two serializations. Each row has exactly one output
/// and one alternate, and the alternate is what the head already emits as
/// `rel="alternate"`, the same set, now a column rather than a scan.
///
/// The pivot describes lands here too: `alternates` is a list of output
/// URLs, and `output.url` is the address, so a relation reading
/// `candidate.output.url` gets the canonical form without knowing an axis
/// exists.
///
/// Mutations, each red: make `join_outputs`' canonical test `true` (the last
/// route minted wins and `alternates` empties, which is exactly the arbitrary
/// pick the closed bug was); make it `false` (no row has an output at all);
/// drop the `alternates.sort()` (the order becomes the axis product's, which
/// is not the order anything else in the database reports).
#[test]
fn an_axis_rows_output_is_canonical_and_the_rest_are_alternates() {
    let dir = site(
        "axis",
        &[
            (
                "grackle.toml",
                br#"
extends = "none"
[site]
url = "https://example.com"
title = "Axes"
author = "Tester"

[schema]
shell = { type = "string" }
theme = { type = "string" }

[[collections]]
source = "."
exclude = ["themes/**"]

  [[collections.rules]]
  match = "look.md"
  front_matter = true
  route = "/{theme}/look/"
  defaults = { shell = "html" }

  [[collections.rules]]
  match = "tiers.md"
  front_matter = true
  route = "/tiers/{serialization}.html"
  defaults = { shell = "html" }

  [[collections.rules]]
  match = "**/*.md"
  front_matter = true
  route = "/{stem}/"
  defaults = { shell = "html" }

  [[collections.rules]]
  match = "**/*"
  route = "/{path}"
  defaults = { shell = "raw" }

[axes.theme]
values = ["default", "loud"]
field = "theme"

[axes.serialization]
values = ["html", "light_html"]
field = "shell"
"#,
            ),
            ("look.md", b"---\ntitle: Look\n---\nTwo looks.\n"),
            ("tiers.md", b"---\ntitle: Tiers\n---\nTwo tiers.\n"),
            ("plain.md", b"---\ntitle: Plain\n---\nOne form.\n"),
            ("themes/loud/theme.scss", b"body { color: red; }\n"),
        ],
    );
    let db = load(&dir);

    let look = row(&db, "look.md");
    assert_eq!(
        output_url(look).as_deref(),
        Some("/default/look/"),
        "the first-declared member is canonical, and that is the output"
    );
    assert_eq!(
        keys(&look.alternates),
        ["/loud/look/"],
        "the other form — what the head emits as rel=alternate"
    );
    // The canonical is `Row.url` too; the join says it rather than assuming it.
    assert_eq!(output_url(look).as_deref(), Some(look.url.as_str()));

    let tiers = row(&db, "tiers.md");
    assert_eq!(output_url(tiers).as_deref(), Some("/tiers/html.html"));
    assert_eq!(keys(&tiers.alternates), ["/tiers/light_html.html"]);

    // A row published once has an output and no alternates, the control that
    // says `alternates` is about the axis and not about routing.
    let plain = row(&db, "plain.md");
    assert_eq!(output_url(plain).as_deref(), Some("/plain/"));
    assert!(plain.alternates.is_empty());

    // The column is real vocabulary, not a debug field: a filter selects the
    // rows that have other forms.
    let multi = Filter::parse("alternates", &grackle_core::model::row_schema()).expect("parses");
    let mut with_forms: Vec<String> = db
        .rows
        .matching(&multi)
        .map(|r| r.rel.to_string_lossy().to_string())
        .collect();
    with_forms.sort();
    assert_eq!(with_forms, ["look.md", "tiers.md"]);

    let _ = std::fs::remove_dir_all(&dir);
}

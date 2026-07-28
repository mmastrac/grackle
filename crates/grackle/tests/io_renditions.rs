//! Renditions as demand-driven outputs (IO.md §4a, item I12).
//!
//! §4a's claim about images is that nothing about them is special: an image is
//! an input row, and a *rendition* of it — a resize, a re-encode — is another
//! **output** of that input, the same way a listing and a feed are outputs of
//! the rows they arrange. What makes a rendition different from every output
//! before it is that it is **transform-bearing**, and the transform has
//! parameters. §4a says where the parameters come from, and it is the whole
//! model in one word: **demand.**
//!
//! So these tests hold five claims:
//!
//! 1. **A rendition is an output with a content edge from its input**, and it
//!    carries the parameters it was made with — `inputs` + `rendition` is the
//!    reproduction recipe;
//! 2. **the citing edge carries the ask**: `{% image … width=N %}` is where the
//!    parameters are written, and two pages asking differently of one image get
//!    two outputs (the demand union);
//! 3. **the hashing law holds at the mint**: the address is a function of the
//!    input bytes and the parameters, so it is nameable at planning — which is
//!    exactly why a page can embed it without waiting for the transform;
//! 4. **the edge runs input → output**, measured: the transform reads the
//!    INPUT's bytes, so renditions do not introduce the output→output content
//!    edge the cycle detector is waiting for;
//! 5. **the demand edge is not a decoration**: an affordance that shows a
//!    rendition and links nothing else still puts the image in the citing
//!    output's `inputs`, or an edit to that image would ship a stale page.
//!
//! Built sites throughout, because a rendition exists only because a citation
//! asked for it and citations live in finished bytes.

use grackle::db::graph::{Demand, Graph, Node};
use grackle::db::{Key, Rendition, SiteDb};
use std::path::{Path, PathBuf};

/// A real PNG, big enough that the transform has something to do — 800×800 of
/// noise, so the resize is visible and the re-encode changes the bytes.
fn png_bytes(w: u32, h: u32, salt: u8) -> Vec<u8> {
    let mut img = image::RgbImage::new(w, h);
    for (i, px) in img.pixels_mut().enumerate() {
        *px = image::Rgb([
            (i % 251) as u8,
            ((i / 7) % 251) as u8,
            salt.wrapping_add((i % 13) as u8),
        ]);
    }
    let dy = image::DynamicImage::ImageRgb8(img);
    let mut buf = Vec::new();
    dy.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .expect("the fixture image encodes");
    buf
}

/// One site with every rendition shape beside each other:
///
/// - `wide.png`, cited by two posts at two different widths (the demand union);
/// - `cover.png`, an image-typed schema field — a HERO, which a listing shows
///   as a thumbnail and links nothing else, so its rendition address is the
///   listing's only reference to it;
/// - `lonely.png`, which nothing cites at all (the pull is the collector).
///
/// `extends = "none"` on purpose, like `io_graph.rs`: one test builds the site
/// twice and diffs the bytes, which a wall-clock feed would break.
const SITE: &str = r#"
extends = "none"
[site]
url = "https://example.com"
title = "Renditions"
author = "Tester"

[schema]
shell = { type = "string" }
draft = { type = "bool" }
cover = { type = "image" }

[[collections]]
name = "objects"
kind = "objects"

  [[collections.rules]]
  match = "pics/**/*.png"
  route = "/{path}"
  on_demand = true
  defaults = { shell = "raw" }

[[collections]]
kind = "tree"
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

# The shape that matters: a card that IS its picture. Its `<a>` links the ROW
# and its `<img>` names the rendition, so `/` cites the cover image through its
# rendition address and through nothing else.
[sets.featured]
from = "entries"
where = 'glob(path, "small.md")'
limit = 1
layout = "card"
variant = "figure"

[profiles.hide]
force = { draft = true }
"#;

fn site(who: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("grackle-io-renditions-{who}"));
    let _ = std::fs::remove_dir_all(&dir);
    let files: &[(&str, Vec<u8>)] = &[
        ("grackle.toml", SITE.as_bytes().to_vec()),
        (
            "index.md",
            b"---\ntitle: Home\n---\nPick of the day:\n\n{% view featured %}\n".to_vec(),
        ),
        (
            "small.md",
            b"---\ntitle: Small\ncover: pics/cover.png\n---\nA narrow one: {% image pics/wide.png width=256 %}\n".to_vec(),
        ),
        (
            "large.md",
            b"---\ntitle: Large\n---\nA wider one: {% image pics/wide.png width=512 %}\n".to_vec(),
        ),
        ("pics/wide.png", png_bytes(800, 800, 0)),
        ("pics/cover.png", png_bytes(800, 800, 40)),
        ("pics/lonely.png", png_bytes(800, 800, 90)),
    ];
    for (rel, body) in files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().expect("a file has a directory")).unwrap();
        std::fs::write(&p, body).unwrap();
    }
    dir
}

fn built(dir: &Path, profile: Option<&str>) -> (grackle::build::SiteOutput, SiteDb) {
    let cfg = grackle::config::Config::load_profile(&dir.join("grackle.toml"), profile)
        .expect("the config loads");
    let mut db = grackle_source::load(&cfg).expect("the site loads");
    let (out, _) = grackle::build::render_site(&cfg, &mut db).expect("the site renders");
    (out, db)
}

fn out(url: &str) -> Node {
    Node::Output(Key::new(url))
}
fn inp(rel: &str) -> Node {
    Node::Input(Key::new(rel))
}

/// Every rendition output in a built database, newest model first: the URL, the
/// inputs it stands on, and the parameters it was made with.
fn renditions(db: &SiteDb) -> Vec<(String, Vec<String>, Rendition)> {
    let mut v: Vec<(String, Vec<String>, Rendition)> = db
        .routes
        .iter()
        .filter_map(|r| {
            r.rendition.map(|p| {
                (
                    r.url.clone(),
                    r.inputs.iter().map(|k| k.to_string()).collect(),
                    p,
                )
            })
        })
        .collect();
    v.sort();
    v
}

// ---------------------------------------------------------------------------

/// **A rendition is an output of its input, carrying its parameters**, and the
/// two together are the reproduction recipe (IO.md §4a).
///
/// The parameters live on the OUTPUT rather than on the edge — review I-D's
/// question, and `rendition.rs`'s module doc carries the argument: a
/// rendition's address hashes the inputs plus the parameters, so every edge
/// arriving at one rendition output carries the same ones by construction, and
/// a slot on the edge would be N copies of one value with nothing keeping them
/// equal.
///
/// What "the pull can reproduce it" means is asserted rather than asserted
/// *about*: read the input's bytes, run the transform with the output's own
/// parameters, and get the published bytes back.
///
/// Mutations, each red and each restored:
///
/// - drop `rendition: Some(*rendition)` from the route `join_renditions` mints
///   → the output is in the graph and nothing in the database says what it is a
///   transform OF, so the reproduction below has no parameters to spend and the
///   `expect` fires: the pull cannot rebuild the variant;
/// - drop `inputs: inputs.clone()` → the rendition stands on nothing, so there
///   is no bytes to reproduce it from and `fanout` loses it (the last test);
/// - drop the `join_renditions` call in `render_site` → the artifacts publish
///   and no output row exists for them at all.
#[test]
fn a_rendition_is_an_output_of_its_input_with_the_parameters_it_was_made_with() {
    let dir = site("recipe");
    let (out_map, db) = built(&dir, None);

    let made = renditions(&db);
    // Three demands: two widths of wide.png, and the hero's default.
    assert_eq!(made.len(), 3, "{made:#?}");

    for (url, inputs, params) in &made {
        // The artifact is published at the output's own URL.
        let published = out_map
            .get(url)
            .unwrap_or_else(|| panic!("{url} publishes bytes"));

        // The recipe: this output's inputs, this output's parameters.
        assert_eq!(inputs.len(), 1, "{url}: {inputs:?}");
        let source = dir.join(&inputs[0]);
        let bytes = std::fs::read(&source).expect("the input is a file");
        let (rebuilt, ext) = grackle::thumbs::render(&bytes, "png", *params)
            .expect("the transform runs from the graph alone");
        assert_eq!(&rebuilt, published, "{url} is reproducible from its edges");

        // …and the address it landed at is the one the recipe names.
        let planned = grackle_source::strong::digest(&bytes, &params.variant());
        assert_eq!(*url, grackle_source::strong::at(&planned, &ext), "{url}");
    }

    // The edge is in the graph, and it is a CONTENT edge: the transform reads
    // the input's bytes, so the input's content stage has to exist first.
    let g = Graph::of(&db);
    let (url, inputs, _) = &made[0];
    let edges: Vec<(Demand, String)> = g
        .needs(&out(url))
        .map(|e| (e.demand, e.from.key().to_string()))
        .collect();
    assert_eq!(edges, vec![(Demand::Content, inputs[0].clone())]);
}

/// **The demand union** (IO.md §4a: "an image's rendition set is the union of
/// its consumers' asks").
///
/// Two pages, two asks, one image: two outputs, one input, and no config
/// anywhere said so. This is §9's q7 settled by what got built — no
/// parameterized shell (`image:256w`), no transform stage upstream of `raw`,
/// no named rendition surface at all. The ask is written at the citation and
/// nowhere else.
///
/// Mutations, each red and each restored:
///
/// - key the rendition map by `src` alone instead of by the whole ask (drop
///   `t.rendition` from `Ask`) → one image is one output whatever anyone asked,
///   and the two widths collapse;
/// - ignore the parsed ask in `tags::image_tag` (always `Rendition::THUMB`) →
///   two demands become one, and `{% image %}` publishes a size nobody wrote.
#[test]
fn two_pages_asking_different_widths_are_two_outputs_of_one_input() {
    let (out_map, db) = built(&site("union"), None);

    let of_wide: Vec<(String, Rendition)> = renditions(&db)
        .into_iter()
        .filter(|(_, ins, _)| ins == &["pics/wide.png".to_string()])
        .map(|(u, _, p)| (u, p))
        .collect();
    assert_eq!(of_wide.len(), 2, "two asks, two outputs: {of_wide:#?}");
    assert_eq!(
        of_wide.iter().map(|(_, p)| *p).collect::<Vec<_>>(),
        {
            let mut want = vec![Rendition::width(256), Rendition::width(512)];
            want.sort_by_key(|p| {
                of_wide
                    .iter()
                    .position(|(_, q)| q == p)
                    .expect("both asks are present")
            });
            want
        },
        "the asks the citations wrote: {of_wide:#?}"
    );

    // Two outputs, and two distinct artifacts — the parameters are in the
    // address, so a rendition can never land on another one.
    assert_ne!(of_wide[0].0, of_wide[1].0);
    assert_ne!(out_map.get(&of_wide[0].0), out_map.get(&of_wide[1].0));

    // …and the pages embed the one they asked for.
    let small = std::str::from_utf8(&out_map["/small/"]).expect("html");
    let large = std::str::from_utf8(&out_map["/large/"]).expect("html");
    let url_of = |p: Rendition| {
        of_wide
            .iter()
            .find(|(_, q)| *q == p)
            .map(|(u, _)| u.clone())
            .expect("the ask has an output")
    };
    assert!(
        small.contains(&format!("src='{}'", url_of(Rendition::width(256)))),
        "{small}"
    );
    assert!(
        large.contains(&format!("src='{}'", url_of(Rendition::width(512)))),
        "{large}"
    );

    // Nothing cited `lonely.png`, so no transform ran and no output exists —
    // §4a's "the pull model is the garbage collector", one transform along.
    assert!(
        !renditions(&db)
            .iter()
            .any(|(_, ins, _)| ins.contains(&"pics/lonely.png".to_string())),
        "an image nothing cites gets no rendition"
    );
}

/// **The hashing law, at site scale** (IO.md §4a): a content-hashed URL hashes
/// the *inputs plus the transform parameters, never the output bytes* — so the
/// address is computable at planning, before the transform runs.
///
/// The consequence is the one this item is really about, and it is asserted
/// here rather than argued: because the address is a planning fact, an output
/// that EMBEDS a rendition depends on the rendition's **facts** and not on its
/// content. That is what keeps the content subgraph bipartite (next test) and
/// what would stop being true the moment an address hashed what a transform
/// produced.
///
/// The extension is the honest exception and it is measured, not hidden: which
/// of {original, PNG, JPEG} wins is a size contest, so the extension is a fact
/// about the output. The digest — the part that makes the address immutable and
/// the part the law is about — is not.
///
/// Mutation, red and restored: hash `out_bytes` instead of the source bytes in
/// `thumbs::one` → the planned digest below stops matching the published
/// address, from a fixture that never runs the transform.
#[test]
fn a_rendition_address_is_computable_before_the_transform_runs() {
    let dir = site("planning");
    let (_, db) = built(&dir, None);

    for (url, inputs, params) in renditions(&db) {
        let bytes = std::fs::read(dir.join(&inputs[0])).expect("the input is a file");
        // Planning: bytes in, parameters in, address out. No decode, no
        // resize, no encoder — this is the whole computation.
        let planned = grackle_source::strong::at(
            &grackle_source::strong::digest(&bytes, &params.variant()),
            "",
        );
        assert!(
            url.starts_with(&planned),
            "{url} is {planned} plus the contest's extension"
        );
        assert!(
            url[planned.len()..].starts_with('.') || url.len() == planned.len(),
            "…and nothing else: {url}"
        );
    }
}

/// **The edge-direction answer, measured** — the question `graph.rs` and
/// `io_graph.rs` both handed to this item.
///
/// I10 armed a cycle detector on the theory that renditions would bring the
/// first output→output CONTENT edge, and I11 confirmed it had not brought one
/// yet. **I12 does not bring one either, and the reason is the hashing law.**
/// A rendition's transform reads the INPUT's bytes — never the original
/// output's — so its content edge runs input → output like every other one.
/// And the citing page reads the rendition's *URL*, which the hashing law makes
/// knowable at planning, so that edge is `Demand::Facts`: a page can render
/// before its thumbnails exist, which is the same sentence as "facts at
/// planning".
///
/// So the content subgraph is still bipartite, `check_acyclic`'s fast path
/// still returns on one linear scan, and the **live cycle fixture is still not
/// expressible** — not for want of trying but because nothing in the engine
/// consumes an output's bytes to make another output. `graph.rs`'s unit tests
/// remain the only place the detector can be seen to fire. Manufacturing a
/// cycle to satisfy a fixture would test the fixture, not the engine.
///
/// Mutations, each red and each restored:
///
/// - push the citing edge into `inputs` instead of `route_members` in
///   `join_renditions`' second half → the demand becomes a content edge, the
///   `Facts` assertion fails, and the bipartite scan below finds the edge;
/// - drop the `route_members` half entirely → the citing page names no
///   rendition at all and the demand is invisible in the graph.
#[test]
fn the_citing_edge_demands_facts_and_the_content_subgraph_stays_bipartite() {
    let (_, db) = built(&site("direction"), None);
    let g = Graph::of(&db);

    let (rendition, _, _) = renditions(&db)
        .into_iter()
        .find(|(_, ins, p)| ins == &["pics/wide.png".to_string()] && *p == Rendition::width(256))
        .expect("the 256 ask has an output");

    // The page that wrote the ask names the rendition, and what it demands of
    // it is FACTS: its address, which planning knew.
    let demand = g
        .needs(&out("/small/"))
        .find(|e| e.from == out(&rendition))
        .map(|e| e.demand);
    assert_eq!(demand, Some(Demand::Facts), "the citing edge");

    // …and the page's own content edges still reach the image, because its
    // bytes hold a string that is a function of that image's bytes.
    assert!(
        g.needs(&out("/small/"))
            .any(|e| e.demand == Demand::Content && e.from == inp("pics/wide.png")),
        "the citing output stands on the input behind the rendition"
    );

    // The measurement: no CONTENT edge anywhere leaves an output. This is the
    // predicate `check_acyclic`'s fast path tests, asserted from outside it and
    // over every output the built site has — renditions included.
    for r in db.routes.iter() {
        for e in g.needed_by(&out(&r.url)) {
            assert_ne!(
                e.demand,
                Demand::Content,
                "an output→output content edge would be the first cycle the engine \
                 could describe: {} → {}",
                e.from.label(),
                e.to.label()
            );
        }
    }
    assert!(g.check_acyclic().is_ok());
}

/// **The demand edge is load-bearing for invalidation**, and this is the guard
/// that says so with bytes — I10's consistency guard, on the shape renditions
/// introduce.
///
/// A LISTING shows each post's hero as a thumbnail and links the POST, not the
/// image: the rendition address is its only reference to that image. Before
/// this item, `/static/{hash}` resolved to nothing in either address index, so
/// the listing's `inputs` lost the image entirely — and editing the image moves
/// the listing's bytes (a new address goes in the `<img src>`) while the graph
/// says it could not have. That is the stale page the graph exists to prevent,
/// and it is a real defect this item closes rather than a hypothetical.
///
/// Asserted the way I10 asserted it: edit one input, rebuild the world, and
/// every output whose bytes moved must lie inside that input's fanout.
///
/// Mutation, red and restored: drop `r.inputs.extend(content)` from
/// `join_renditions`' second half → `/list/` moves and is not in the fanout,
/// which is exactly the failure a stale incremental rebuild would ship. (The
/// post's own page does NOT go red on that mutation, and that is worth
/// recording: `{% image %}` links the original beside its thumbnail, so
/// `join_citations` re-derives the same edge from the anchor. The listing is
/// the shape with no anchor to fall back on.)
#[test]
fn an_output_that_only_shows_a_rendition_still_depends_on_the_image() {
    let dir = site("fanout");
    let (before, db) = built(&dir, None);

    let g = Graph::of(&db);
    let fan: Vec<String> = g
        .fanout(&inp("pics/cover.png"))
        .iter()
        .map(|n| n.key().to_string())
        .collect();
    assert!(
        fan.iter().any(|u| u == "/index/"),
        "the card shows this image's rendition and nothing else of it: {fan:?}"
    );
    assert!(
        !fan.iter().any(|u| u == "/large/"),
        "and not everything: {fan:?}"
    );

    // Move the image and rebuild the world.
    std::fs::write(dir.join("pics/cover.png"), png_bytes(800, 800, 137)).unwrap();
    let (after, _) = built(&dir, None);

    let moved: Vec<&String> = after
        .iter()
        .filter(|(u, b)| before.get(*u).is_none_or(|old| old != *b))
        .map(|(u, _)| u)
        .collect();
    assert!(!moved.is_empty(), "the edit moved something");
    for u in &moved {
        // A rendition's own address moves with its input, so the new artifact
        // is a new URL rather than a changed one; what must be in the fanout is
        // every EXISTING output that moved.
        if before.contains_key(*u) {
            assert!(
                fan.contains(u),
                "{u} moved but is not in the fanout {fan:?} — a missing edge"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// **Rung 0 reaches the third minting seam** (IO.md I10's law: minting an
/// output is the graph event, so every seam that mints one applies the
/// profile's forced fields).
///
/// I10 closed this at `materialize_referenced` and said the hole grows once per
/// minting seam; I11 added a shape to that seam rather than a seam. I12 adds
/// the seam, and the law is why that cost one loop instead of a rediscovery.
///
/// Byte-inert today, exactly as it was at I10 — a rendition is a byte publish
/// with no head — and asserted so that the next seam finds the law already
/// applied rather than already broken.
///
/// Mutation, red and restored: delete the `forced` loop in `join_renditions` →
/// the rendition carries nothing while every other route on the same build
/// carries `draft`.
#[test]
fn a_rendition_minted_at_build_sees_rung_zero() {
    let (_, db) = built(&site("rung0"), Some("hide"));
    let (url, _, _) = renditions(&db).into_iter().next().expect("a rendition");
    assert_eq!(
        db.routes
            .get(&Key::new(&url))
            .expect("the route")
            .fields
            .get("draft"),
        Some(&grackle::filter::Value::Bool(true)),
        "the profile forces `draft`, and a minted rendition is a route"
    );

    // The control: the value is the PROFILE's, not a default.
    let (_, plain) = built(&site("rung0-plain"), None);
    let (url, _, _) = renditions(&plain).into_iter().next().expect("a rendition");
    assert_eq!(
        plain
            .routes
            .get(&Key::new(&url))
            .expect("the route")
            .fields
            .get("draft"),
        None
    );
}

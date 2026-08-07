//! The graph.
//!
//! Not a new fact in the database: a join gave every output the two columns
//! that name what it stands on (`inputs`, `route_members`), and this is those
//! columns read as nodes and edges. So what these tests hold is not "the graph
//! is correct", that would be re-asserting the join, but the four claims the
//! *reading* makes:
//!
//! 1. **one graph, two edge kinds**: a fold's edges are BOTH, and a listing's
//!    are one, and the labels say which stage each demands;
//! 2. **the pull orders**: dependencies before dependents, the output last;
//! 3. **the cycle tripwire is armed and cannot fire**, because a pool fold
//!    selecting its own route is a real corpus shape and a legal one, which is
//!    true only while the two edge kinds are told apart;
//! 4. **`materialize_referenced` is a pull along the edges**: a citation is an
//!    edge, and the output it mints is a node like any other, rung 0
//!    included.
//!
//! Built sites where the claim needs bytes (the citation edge exists only
//! after the write pass; the minted output only after the pull), loaded sites
//! elsewhere.

use grackle_core::model::graph::{Demand, Graph, Node};
use grackle_core::model::Key;
use std::path::PathBuf;

mod support;
use support::{load, render_profile as built};

/// A 2×3 PNG, real bytes, so an image row is an image row.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x08, 0x02, 0x00, 0x00, 0x00, 0x36, 0x88, 0x49,
    0xd6, 0x00, 0x00, 0x00, 0x16, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x60, 0x10, 0x91, 0x03,
    0x21, 0x2e, 0x11, 0x39, 0x2e, 0x10, 0x4b, 0x44, 0x0e, 0x88, 0x00, 0x0d, 0x49, 0x01, 0x69, 0x37,
    0x8b, 0x8f, 0x82, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// One site with all four shapes beside each other: two documents, a listing
/// that arranges them, a pool fold that arranges every OUTPUT (including its
/// own, the facts cycle), a byte copy, and two on-demand images of which
/// exactly one is cited.
///
/// `extends = "none"` on purpose: the base's feed carries a wall-clock
/// `<updated>`, and one of these tests builds the same site twice and diffs
/// the bytes.
const SITE: &str = r#"
extends = "none"
[site]
url = "https://example.com"
title = "Graph"
author = "Tester"

[schema]
shell = { type = "string" }
draft = { type = "bool" }

[[collections]]
name = "objects"

  # on-demand: the URL is computed at load, the output is minted only when
  # the pull reaches it. grack.com's own images rule, minimised.
  [[collections.rules]]
  match = "pics/**/*.png"
  route = "/{path}"
  on_demand = true
  defaults = { shell = "raw" }

[[collections]]
name = "entries"
source = "."

  [[collections.rules]]
  match = { pattern = "**/*.md", front_matter = true }
  route = "/{stem}/"
  defaults = { shell = "html" }

  [[collections.rules]]
  match = "**/*"
  route = "/{path}"
  defaults = { shell = "raw" }

# A listing: its edges are its MEMBERS, rows every one of them.
[views.list]
path = "/list/"
from = "entries"
where = 'shell == "html"'
layout = "card"
title = "List"

# A fold over the output pool with no `where`: it selects every route,
# its own included. Both edge kinds meet here.
[views.all]
path = "/all.xml"
shell = "sitemap"

# A fold that reads member CONTENT, and publishes BINARY — so it is the one
# output whose dependence on a row cannot also arrive as a citation.
[views.search]
path = "/search.bin"
shell = "search"
where = 'shell == "html"'

[profiles.hide]
force = { draft = true }
"#;

fn site(who: &str) -> PathBuf {
    let files: &[(&str, &[u8])] = &[
        ("grackle.toml", SITE.as_bytes()),
        (
            "a.md",
            b"---\ntitle: Ay\n---\nProse, and a picture: ![kite](/pics/kite.png)\n",
        ),
        ("b.md", b"---\ntitle: Bee\n---\nMore prose.\n"),
        ("notes.txt", b"Bytes, verbatim.\n"),
        ("pics/kite.png", PNG),
        ("pics/unseen.png", PNG),
    ];
    support::site("io-graph", who, files)
}

fn out(url: &str) -> Node {
    Node::Output(Key::new(url))
}
fn inp(rel: &str) -> Node {
    Node::Input(Key::new(rel))
}

/// The edges into one node, as `("content"|"facts", label)` pairs, sorted so a
/// test compares sets rather than construction order.
fn needs(g: &Graph, n: &Node) -> Vec<(&'static str, String)> {
    let mut v: Vec<(&'static str, String)> = g
        .needs(n)
        .map(|e| {
            let kind = match e.demand {
                Demand::Content => "content",
                Demand::Facts => "facts",
            };
            (kind, e.from.key().to_string())
        })
        .collect();
    v.sort();
    v
}

// ---------------------------------------------------------------------------

/// **One graph, two edge kinds**, decided and asserted.
///
/// The two columns name keys in different stores, and the reason they are one
/// graph rather than two is visible here: `/all.xml` carries BOTH kinds at
/// once, so a traversal that knew only one of them could not materialize it.
/// A listing carries only content edges, and a document only one, which is
/// what makes the fold's pair a distinction rather than a decoration.
///
/// What each label MEANS is the other half: a content edge says the
/// dependent's bytes read the dependency, a facts edge says it reads only what
/// planning already knows. The fold's facts edges point at OUTPUTS and its
/// content edges at the rows behind them, the same edge followed one step
/// further into the inputs database, which is what `join_arrangement` means.
///
/// Mutations, each red and each restored:
///
/// - drop the `route_members` loop from `graph::Graph::of` -> the fold's facts
///   edges vanish and the two-kinds assertion fails, while every content edge
///   still stands (the "two graphs" reading, seen);
/// - drop the `inputs` loop -> every content edge goes, and with it the pull.
#[test]
fn one_graph_holds_both_edge_kinds() {
    let db = load(&site("edges"));
    let g = Graph::of(&db);

    // A document: one content edge, from the row it renders.
    assert_eq!(
        needs(&g, &out("/a/")),
        vec![("content", "a.md".to_string())]
    );

    // A listing: content edges, one per member, and nothing else. Membership
    // is arrangement of INPUTS, so there is no facts edge to have.
    assert_eq!(
        needs(&g, &out("/list/")),
        vec![
            ("content", "a.md".to_string()),
            ("content", "b.md".to_string())
        ]
    );

    // The fold over the output pool: both kinds. The facts edges name every
    // route it selected, its own included, which is the shape the next test
    // is about, and the content edges name the rows behind them.
    let fold = needs(&g, &out("/all.xml"));
    let facts: Vec<&String> = fold
        .iter()
        .filter(|(k, _)| *k == "facts")
        .map(|(_, u)| u)
        .collect();
    let content: Vec<&String> = fold
        .iter()
        .filter(|(k, _)| *k == "content")
        .map(|(_, u)| u)
        .collect();
    assert!(
        facts.iter().any(|u| *u == "/list/") && facts.iter().any(|u| *u == "/a/"),
        "the fold arranges outputs: {facts:?}"
    );
    assert!(
        content.iter().any(|u| *u == "a.md") && content.iter().any(|u| *u == "notes.txt"),
        "and holds the rows behind them: {content:?}"
    );
    assert!(
        !facts.is_empty() && !content.is_empty(),
        "ONE graph: a fold's edges are both kinds"
    );

    // Both stores are nodes whether or not anything joins them: the uncited
    // image is an input with no output at all, and it is still in the graph.
    assert!(g.has(&inp("pics/unseen.png")));
    assert_eq!(needs(&g, &inp("pics/unseen.png")), vec![]);
}

/// **The pull**: dependencies before dependents, the output last.
///
/// Tested standalone, because that is the honest shape of this item: `serve`
/// still rebuilds the world, so the entry point exists and has no
/// caller yet. A test is what keeps it from being a claim.
///
/// The fold is where the two labels do different work. A **content** edge
/// recurses, whatever the dependency itself needs comes before it. A
/// **facts** edge does not: planning finished those, so the member appears as
/// a prerequisite and the walk stops. That is why `/all.xml`, which selects
/// its own route, yields a finite order at all.
///
/// Mutation, red and restored: reverse the content edge in `Graph::of`
/// (`g.add(to, from, …)`) -> `/a/` pulls nothing and the row is the dependent,
/// so the order assertion fails on the very first case.
///
/// **And one decision recorded rather than pinned**, because no mutation can
/// make it red today: making `pull` recurse into facts edges as well leaves
/// every order here unchanged (measured). It cannot differ while a fold's
/// `inputs` already holds the rows behind its members, the recursion re-finds
/// exactly what the fold's own content edges named. So the non-recursion is
/// the label's MEANING rather than an observable behaviour, and where the
/// label does earn its keep is the next test: at the cycle check, where the
/// same self-edge is a legal site or a refused one depending on it.
#[test]
fn the_pull_orders_dependencies_before_dependents() {
    let db = load(&site("pull"));
    let g = Graph::of(&db);

    // A document: its row, then it.
    assert_eq!(g.pull(&out("/a/")), vec![inp("a.md"), out("/a/")]);

    // A listing: every member before it, it last.
    let order = g.pull(&out("/list/"));
    assert_eq!(order.last(), Some(&out("/list/")), "{order:?}");
    for m in [inp("a.md"), inp("b.md")] {
        let at = order.iter().position(|n| *n == m).expect("a member");
        assert!(at < order.len() - 1, "{m:?} comes before the listing");
    }

    // The fold: finite, ends at itself, and its own facts edge is not a step.
    let order = g.pull(&out("/all.xml"));
    assert_eq!(order.last(), Some(&out("/all.xml")), "{order:?}");
    assert_eq!(
        order.iter().filter(|n| **n == out("/all.xml")).count(),
        1,
        "a fold that selects itself appears once: {order:?}"
    );
    assert!(order.contains(&out("/list/")), "{order:?}");
    assert!(order.contains(&inp("a.md")), "{order:?}");

    // An output the graph does not hold gets an answer, not a panic.
    assert_eq!(g.pull(&out("/nowhere/")), Vec::<Node>::new());
}

/// **The tripwire, and why it cannot fire.**
///
/// A pool fold with no `where` selects every route including its own, this is
/// not a hypothetical, it is what `io_folds.rs` asserts in `<loc>` form and
/// what `/all.xml` does here. Read as one undifferentiated graph, that is a
/// cycle, and every site that writes such a fold would stop loading. Read with
/// the labels, it is a facts edge: the fold needs its member's URL and shell,
/// which planning finished before any content existed, so there is nothing to
/// order and nothing to refuse.
///
/// So the load-time check (`load::check_graph`) passes on every corpus site
/// for a structural reason rather than by luck: content edges run input ->
/// output, and an input has no incoming edge, so the content subgraph is
/// bipartite and has no cycle to find. The detector itself is exercised where
/// a cycle can actually be built, `graph.rs`'s unit tests, which hand it an
/// output->output content edge by hand. **Nothing introduced one**:
/// a strong address hashes an INPUT's bytes, and so does a rendition's
/// transform, while the page that EMBEDS a rendition reads only its
/// address, which the hashing law makes a planning fact, so that edge is
/// `Facts`. Both measured; `graph.rs` carries the argument and
/// `io_renditions.rs` asserts the predicate over a whole built site.
///
/// Mutations, each red and each restored:
///
/// - label `route_members` as `Demand::Content` in `Graph::of` -> this site
///   stops loading with *dependency cycle: output /all.xml -> output /all.xml*,
///   which is the single measurement that decides the edge-kind split. (The
///   real corpus does NOT tell them apart: grack.com's sitemap says
///   `dir || ext == "html"`, so it excludes its own `.xml` route. A from-less
///   fold is what makes the loop, and `io_folds.rs`'s three tests go red on
///   the same mutation for the same reason.)
/// - delete `check_graph`'s call in `load` alone -> nothing goes red, because
///   nothing can build a cycle today; delete it TOGETHER with the mislabel
///   above and the site loads clean, publishing a fold that is its own content
///   dependency. That pair is the call site's mutation: the check is what
///   turns a mislabelled edge from a silent build into a load error, and the
///   detector's own guards are mutated where a cycle can be built at all
///   (`graph.rs`'s unit tests).
#[test]
fn a_pool_fold_that_selects_itself_is_not_a_cycle() {
    let dir = site("cycle");
    let db = load(&dir);

    // Measured, not assumed: the fold really is its own member.
    let fold = db
        .routes
        .get(&Key::new("/all.xml"))
        .expect("the fold has a route");
    assert!(
        fold.route_members.contains(&Key::new("/all.xml")),
        "the pool includes the fold's own route: {:?}",
        fold.route_members
    );

    // …and the graph says so, as a FACTS edge, and is acyclic all the same.
    let g = Graph::of(&db);
    assert!(g
        .needs(&out("/all.xml"))
        .any(|e| e.from == out("/all.xml") && e.demand == Demand::Facts));
    assert!(
        g.check_acyclic().is_ok(),
        "a facts loop is not a materialization cycle"
    );
}

/// **`materialize_referenced` is a pull along the graph's edges**.
///
/// A citation names a URL; `db.by_url` is the inputs database's address index,
/// so resolving one is walking a content edge to the input at its far end. An
/// input that publishes on demand and has no `output` yet is a node the pull
/// has reached and planning did not materialize, so the pass mints its output
/// and the edge to it. The `output` column IS the "already done" test, which
/// is the private index this item deleted.
///
/// The uncited twin is the control, and it is what makes this a statement
/// about edges rather than about images: same rule, same extension, same
/// directory, no edge, no output
///
/// Mutations, each red and each restored:
///
/// - seed `frontier` with an empty vector instead of `cited` -> the edge is
///   dropped and the cited asset is not published, while the citing page still
///   ships with a link to nothing (a dropped edge, seen from the output side);
/// - drop the `row.output.is_some()` half of the guard -> the asset is cited
///   from two finished documents, so it is minted TWICE: two outputs at one
///   URL, and the graph shows the doubled edge (measured, not predicted, the
///   old private `pending` map answered this question by removal, and the
///   `output` column answers it by being set);
/// - drop the minted route's `inputs: vec![key]` -> the asset publishes with no
///   content edge, so its own output stands on nothing and `fanout` loses it.
#[test]
fn the_pull_publishes_a_cited_input_and_only_a_cited_one() {
    let dir = site("materialize");
    let (out_map, db) = built(&dir, None);

    assert!(
        out_map.contains_key("/pics/kite.png"),
        "the citation pulled it"
    );
    assert!(
        !out_map.contains_key("/pics/unseen.png"),
        "nothing cited it"
    );

    // The minted output is a node like any other, with the content edge that
    // says what it came from.
    let g = Graph::of(&db);
    assert_eq!(
        needs(&g, &out("/pics/kite.png")),
        vec![("content", "pics/kite.png".to_string())]
    );
    assert!(!g.has(&out("/pics/unseen.png")), "no output, no node");

    // And the citing page gained the citation edge the render pass discovered
    // (`join_citations`), the half of the closure only content can answer.
    assert!(
        needs(&g, &out("/a/")).contains(&("content", "pics/kite.png".to_string())),
        "{:?}",
        needs(&g, &out("/a/"))
    );
}

/// **The rung-0 seam, closed**.
///
/// `force_route_fields` cannot reach a route minted after the
/// load returns. With the graph the shape has a name:
/// minting an output is a graph event, so rung 0 belongs at every seam that
/// mints one rather than at the one pass that happened to run first. The typed
/// values are computed once at load (`SiteDb::forced_fields`) and applied at
/// both seams, one list, two writers.
///
/// Byte-inert today, and that is stated rather than hidden: these routes are
/// byte publishes with no head, minted below every reader of a route field. It
/// is closed now because the hole grows every time an output is minted at a
/// new seam. Strong addressing added a SHAPE to this seam rather than a seam,
/// its strong mint sits inside the same loop, under the same `forced_fields`,
/// which is the cheapest way there is to stay inside the law.
///
/// Mutations, each red and each restored: delete the `forced` loop in
/// `materialize_referenced` (the minted route carries nothing while every
/// other route on the same build carries `draft`); delete
/// `db.forced_fields = …` in `force_route_fields` (the same, from the other
/// end, and the route half still passes, which is what makes the two lines
/// separately required).
#[test]
fn an_output_minted_by_the_pull_sees_rung_zero() {
    let dir = site("rung0");
    let (_, db) = built(&dir, Some("hide"));
    let minted = db
        .routes
        .get(&Key::new("/pics/kite.png"))
        .expect("the pull minted it");
    assert_eq!(
        minted.fields.get("draft"),
        Some(&grackle_core::filter::Value::Bool(true)),
        "the profile forces `draft`, and a route minted by the pull is a route"
    );
    // The control: the forced value is the PROFILE's, not a default, the
    // default build's minted route carries no `draft` at all.
    let (_, plain) = built(&site("rung0-plain"), None);
    assert_eq!(
        plain
            .routes
            .get(&Key::new("/pics/kite.png"))
            .expect("minted here too")
            .fields
            .get("draft"),
        None
    );
}

/// **Invalidation is a traversal of this graph**,
/// and this is the consistency guard that says so with bytes rather than with
/// prose.
///
/// design typed invalidation keys are a design, not machinery: `serve`
/// rebuilds the world today, so there is no live key set to compare a fanout
/// against. What CAN be compared is the fanout against reality, edit one
/// input, rebuild, and every output whose bytes moved must be inside
/// `fanout(that input)`. A missing edge is exactly an output that moves and is
/// not in the set, which is the failure an incremental rebuild would ship as a
/// stale page.
///
/// The fanout is asserted non-trivial first, because a guard whose subset test
/// passes by being everything is not a guard.
///
/// Mutations, each red and each restored:
///
/// - drop the `if let Some(k) = &r.row` term in `load::join_arrangement` ->
///   `/a/` leaves the fanout while its own bytes move: the stale page, caught;
/// - drop the `for rk in &r.route_members` term -> `/search.bin` leaves, and it
///   is the output that proves the term, because a search index is BINARY: it
///   cites nothing, so the citation half of the closure cannot cover for the
///   arrangement half the way it does for a listing.
///
/// And one mutation that is NOT red, measured rather than predicted: dropping
/// `ins.extend(r.members…)` leaves this test green, because a listing LINKS
/// what it arranges, so `join_citations` re-derives the same edge from the
/// finished bytes. Arrangement and citation genuinely overlap wherever an
/// arrangement is rendered as links, which is why `viewed_by` and
/// `linked_from` are still two fields (they answer different questions) and
/// why `inputs` may hold one edge from two sources without double-counting.
#[test]
fn a_changed_inputs_bytes_move_only_inside_its_fanout() {
    let dir = site("fanout");
    let (before, db) = built(&dir, None);

    let g = Graph::of(&db);
    let fan: Vec<String> = g
        .fanout(&inp("a.md"))
        .iter()
        .map(|n| n.key().to_string())
        .collect();
    for expect in ["/a/", "/list/", "/all.xml", "/search.bin"] {
        assert!(fan.iter().any(|u| u == expect), "{expect} in {fan:?}");
    }
    assert!(
        !fan.iter().any(|u| u == "/b/"),
        "and not everything: {fan:?}"
    );

    // Now move that input and rebuild the world.
    std::fs::write(
        dir.join("a.md"),
        b"---\ntitle: Ay, revised\n---\nProse, and a picture: ![kite](/pics/kite.png)\n",
    )
    .unwrap();
    let (after, _) = built(&dir, None);

    let moved: Vec<&String> = after
        .iter()
        .filter(|(u, b)| before.get(*u).is_none_or(|old| old != *b))
        .map(|(u, _)| u)
        .collect();
    assert!(!moved.is_empty(), "the edit moved something");
    for u in &moved {
        assert!(
            fan.contains(u),
            "{u} moved but is not in the fanout {fan:?} — a missing edge"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

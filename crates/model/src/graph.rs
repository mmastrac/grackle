//! IO.md §5: **the graph** — the two databases, joined, read as nodes and edges.
//!
//! The site is inputs and outputs (IO.md §1); I9 gave each side the columns
//! that name the other (`Route.inputs`, `Route.route_members`). This module is
//! those columns read as a graph, and it adds no facts of its own: it is a
//! *view* of the join, constructed at planning time from a finished `SiteDb`,
//! and every edge in it is a key already sitting in a column. That is what
//! makes it cheap enough to build on every load and impossible to drift.
//!
//! **One graph, two edge kinds** (IO.md I9's flag 5, decided here). The two
//! columns name keys in different stores — `inputs` names rows, `route_members`
//! names routes — but they are the same graph, because the pull that
//! materializes a fold has to traverse both: `/sitemap.xml` reads the routes it
//! selected, and the rows behind them. Two graphs would mean two traversals
//! that must agree, which is the shape a join exists to delete. So the edges
//! are labelled instead, and what the label says is the law of §1:
//!
//! > **Facts at planning; content at materialization.**
//!
//! A [`Demand::Content`] edge says the dependent's *bytes* read the
//! dependency — so the dependency's content stage must be forced first, and
//! order matters. A [`Demand::Facts`] edge says the dependent reads only what
//! planning already knows (url, shell, flags) — so it constrains nothing,
//! because facts are complete for every output before any content exists.
//!
//! **Which is why a cycle cannot be expressed today, and the check is still
//! here.** Content edges run input → output only: an output's bytes are read
//! by nobody, because nothing in the engine yet derives an output from another
//! output's *content*. The content subgraph is therefore bipartite, and a
//! bipartite graph with all its sources on one side has no cycles to find.
//! Facts edges are the ones that can loop — measured, not reasoned: a pool
//! fold with no `where` selects its own route, so `/all.xml` is genuinely its
//! own `route_members` member — and that is legal precisely because a facts
//! edge demands nothing that has to be produced first (IO.md §4's column
//! rule: the sitemap reads facts, so it may list fold products; search reads
//! content, so it sees only content-bearing outputs). [`Graph::check_acyclic`]
//! looks at the content subgraph alone, and it is armed rather than
//! decorative: a transform that derives an output from *another output's
//! bytes* is the first content edge that can point output → output, and the
//! first way a config could describe a cycle.
//!
//! **Neither I11 nor I12 brought one, and both were measured rather than
//! hoped** (IO.md §4a). A strong address publishes an INPUT's bytes at a hash
//! of those bytes: the output it mints carries `inputs = [that row]`, so its
//! content edge runs input → output like every other, and the untransformed
//! twin adds a second input to one output rather than an output to an output.
//!
//! **I12's renditions were expected to be the exception and are not, and the
//! reason is the hashing law.** A rendition is a transform of an input, and the
//! transform reads the INPUT's bytes — `thumbs::render` is a function of the
//! source file and the ask — so its content edge runs input → output too. The
//! citing page is the other candidate, and it is a [`Demand::Facts`] edge: a
//! page embeds a rendition's *address*, and because that address hashes the
//! inputs plus the parameters (never the output bytes) it is knowable at
//! planning, so the page can materialize before the transform has run. That is
//! §1's law doing load-bearing work rather than describing itself: **the
//! hashing law is exactly what keeps the demand edge a facts edge**, and an
//! address computed from what a transform produced would turn it into a content
//! edge and make the first cycle describable.
//!
//! So the bipartite argument still holds whole, the fast path below still
//! returns on one linear scan, and the unit-level fixture that hands
//! `from_edges` a real output → output cycle remains the only place the
//! detector can be seen to fire. **The live fixture through `Config::load` is
//! not owed by any item on the ledger** — it is not expressible until something
//! in the engine consumes an OUTPUT's bytes to make another output, and nothing
//! does. Manufacturing one would test the fixture rather than the engine.
//! `io_renditions.rs` asserts the predicate over a whole built site, which is
//! the honest form of the claim.

pub use crate::{Demand, Edge, Node};

use crate::SiteDb;
use std::collections::{HashMap, HashSet};

/// The dependency graph of one planned site.
#[derive(Debug, Default)]
pub struct Graph {
    edges: Vec<Edge>,
    /// Edge indices by dependent — "what does this node need".
    into: HashMap<Node, Vec<usize>>,
    /// Edge indices by dependency — "what needs this node".
    from: HashMap<Node, Vec<usize>>,
    nodes: HashSet<Node>,
}

impl Graph {
    /// Build the graph of a planned site: nodes from facts-at-planning, edges
    /// from the join's columns.
    ///
    /// - every row is an `Input` node, every route an `Output` node — the two
    ///   databases, whether or not anything joins them;
    /// - `Route.inputs` gives the **content** edges (IO.md I9's full row-level
    ///   closure: the row a route renders, a landing's claimed body, a view's
    ///   members, the rows behind a fold's selected routes, and — after the
    ///   write pass — every row the finished bytes cite);
    /// - `Route.route_members` gives the **facts** edges, the output→output
    ///   half: the routes a pool fold arranged, and (I12) the renditions an
    ///   output's finished bytes embedded — both cases where one output reads
    ///   another's planning facts and none of its content.
    ///
    /// Nothing is recomputed and nothing is looked up: both columns already
    /// hold keys, which is what I9 bought and why this costs one pass.
    pub fn of(db: &SiteDb) -> Graph {
        let mut g = Graph::default();
        for row in db.rows.iter() {
            g.nodes.insert(Node::Input(row.key.clone()));
        }
        for r in db.routes.iter() {
            let to = Node::Output(r.id.clone());
            g.nodes.insert(to.clone());
            for k in &r.inputs {
                g.add(Node::Input(k.clone()), to.clone(), Demand::Content);
            }
            for k in &r.route_members {
                g.add(Node::Output(k.clone()), to.clone(), Demand::Facts);
            }
        }
        g
    }

    /// Build a graph from an explicit edge list. The construction the engine
    /// cannot yet describe — an output whose *content* is another output's —
    /// is reachable only this way, which is how [`check_acyclic`] is tested at
    /// all.
    ///
    /// [`check_acyclic`]: Graph::check_acyclic
    pub fn from_edges(edges: impl IntoIterator<Item = (Node, Node, Demand)>) -> Graph {
        let mut g = Graph::default();
        for (from, to, demand) in edges {
            g.add(from, to, demand);
        }
        g
    }

    fn add(&mut self, from: Node, to: Node, demand: Demand) {
        let i = self.edges.len();
        self.nodes.insert(from.clone());
        self.nodes.insert(to.clone());
        self.into.entry(to.clone()).or_default().push(i);
        self.from.entry(from.clone()).or_default().push(i);
        self.edges.push(Edge { from, to, demand });
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn has(&self, n: &Node) -> bool {
        self.nodes.contains(n)
    }

    /// What this node needs — its incoming edges, in construction order.
    pub fn needs(&self, n: &Node) -> impl Iterator<Item = &Edge> {
        self.into
            .get(n)
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .map(|i| &self.edges[*i])
    }

    /// What needs this node — its outgoing edges, in construction order.
    pub fn needed_by(&self, n: &Node) -> impl Iterator<Item = &Edge> {
        self.from
            .get(n)
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .map(|i| &self.edges[*i])
    }

    /// **Invalidation, as a traversal** (IO.md §5's first view): every output
    /// whose bytes could move because this node changed, transitively, over
    /// CONTENT edges.
    ///
    /// This is the set the incremental machinery's typed `Row(path)` keys have
    /// been curating by hand — DESIGN §2 — said once, as a walk of the join.
    /// Facts edges are deliberately not followed: a pool fold that arranged an
    /// output already holds the rows behind it in its own `inputs` (that is
    /// what `join_arrangement` means by following the edge one step further
    /// into the inputs database), so following the facts edge as well would
    /// count the same dependency twice without adding one.
    ///
    /// Sorted, so a caller comparing two fanouts compares sets rather than
    /// walk orders.
    pub fn fanout(&self, n: &Node) -> Vec<Node> {
        let mut seen: HashSet<Node> = HashSet::new();
        let mut stack = vec![n.clone()];
        while let Some(cur) = stack.pop() {
            for e in self.needed_by(&cur) {
                if e.demand != Demand::Content {
                    continue;
                }
                if seen.insert(e.to.clone()) {
                    stack.push(e.to.clone());
                }
            }
        }
        let mut out: Vec<Node> = seen.into_iter().collect();
        out.sort();
        out
    }

    /// **The pull** (IO.md §1: build is "pull every output", serve is "pull
    /// this one"): the work of materializing one output, dependencies before
    /// dependents, the output itself last.
    ///
    /// A **content** edge recurses — the dependency's own content has to exist
    /// first, so its own dependencies come before it. A **facts** edge does
    /// not: the label says the dependent reads planning facts, and those are
    /// complete for every output before the pull starts, so the member appears
    /// as a prerequisite and the walk stops there. That keeps a fold's pull
    /// linear in its member count rather than in the closure behind them.
    ///
    /// **Recorded rather than pinned** (io_graph.rs): recursing into facts
    /// edges too produces the same order on every shape the engine can build,
    /// because a fold's `inputs` already holds the rows behind its members —
    /// the recursion would re-find exactly what the content edges named. So
    /// the non-recursion is what the label MEANS here; where it is observable
    /// is [`Graph::check_acyclic`], which is why a pool fold selecting itself
    /// is a legal site.
    ///
    /// Returns the empty vector for a node the graph does not hold, so a
    /// caller asking about a URL that is not an output gets an answer rather
    /// than a panic.
    pub fn pull(&self, target: &Node) -> Vec<Node> {
        if !self.has(target) {
            return Vec::new();
        }
        // Iterative post-order DFS: push a node, then its unvisited content
        // dependencies; emit when its dependencies have all been emitted.
        let mut out: Vec<Node> = Vec::new();
        let mut done: HashSet<Node> = HashSet::new();
        let mut open: HashSet<Node> = HashSet::new();
        let mut stack: Vec<(Node, bool)> = vec![(target.clone(), false)];
        while let Some((n, expanded)) = stack.pop() {
            if done.contains(&n) {
                continue;
            }
            if expanded {
                done.insert(n.clone());
                out.push(n);
                continue;
            }
            open.insert(n.clone());
            stack.push((n.clone(), true));
            for e in self.needs(&n) {
                if e.from == n || done.contains(&e.from) {
                    continue; // a self-edge is not a prerequisite of itself
                }
                match e.demand {
                    // A facts prerequisite is a leaf: planning already
                    // finished it, so it needs nothing of its own here.
                    Demand::Facts => {
                        if !done.contains(&e.from) && !open.contains(&e.from) {
                            done.insert(e.from.clone());
                            out.push(e.from.clone());
                        }
                    }
                    Demand::Content => stack.push((e.from.clone(), false)),
                }
            }
        }
        out
    }

    /// The tripwire: no output's CONTENT may depend, transitively, on its own.
    ///
    /// Checked at load (IO.md §5's "cycle detection at load — a config error,
    /// never a render surprise", the relations precedent) and, today, a check
    /// that cannot fire: content edges run input → output, inputs have no
    /// incoming edges, and a graph whose every source is on one side of a
    /// bipartition has no cycle to find. I12 measured the case this comment
    /// used to name — a rendition derives from its INPUT's bytes, not from
    /// another output's — so the check is still armed and still unfireable, and
    /// the day that changes is the day something consumes an output's bytes.
    /// A check written then is a check written after the first render surprise.
    ///
    /// Returns the cycle in dependency order when there is one, which is what
    /// makes the error a diagnosis rather than a complaint.
    pub fn check_acyclic(&self) -> Result<(), Vec<Node>> {
        // **The bipartite argument, executable.** A cycle needs a content edge
        // leaving an output; today every content edge leaves an INPUT, and an
        // input has no incoming edge of any kind, so one linear scan settles
        // the whole question and the sort below never runs on a corpus site.
        // This is the doc comment's claim as a fast path rather than beside
        // one — when it stops being true, the scan finds the edge and the real
        // check takes over. (I12 was the item expected to make it stop being
        // true, and measured otherwise: a rendition reads its INPUT's bytes,
        // and the page that embeds it reads only its address.)
        if !self
            .edges
            .iter()
            .any(|e| e.demand == Demand::Content && matches!(e.from, Node::Output(_)))
        {
            return Ok(());
        }
        // A self-edge is its own cycle, and Kahn cannot see one — it never
        // raises anything's in-degree — so it is asked first rather than
        // after, which is where the first draft had it and where the unit
        // test caught it.
        if let Some(e) = self
            .edges
            .iter()
            .find(|e| e.demand == Demand::Content && e.from == e.to)
        {
            return Err(vec![e.from.clone()]);
        }
        // Kahn over the content subgraph: whatever never reaches in-degree
        // zero is in (or downstream of) a cycle. Then one walk to name it.
        let mut indeg: HashMap<&Node, usize> = self.nodes.iter().map(|n| (n, 0)).collect();
        for e in &self.edges {
            if e.demand == Demand::Content && e.from != e.to {
                *indeg.entry(&e.to).or_insert(0) += 1;
            }
        }
        let mut ready: Vec<&Node> = indeg
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(n, _)| *n)
            .collect();
        let mut settled = 0usize;
        while let Some(n) = ready.pop() {
            settled += 1;
            for e in self.needed_by(n) {
                if e.demand != Demand::Content || e.from == e.to {
                    continue;
                }
                let d = indeg.get_mut(&e.to).expect("every node has a degree");
                *d -= 1;
                if *d == 0 {
                    ready.push(&e.to);
                }
            }
        }
        if settled == self.nodes.len() {
            return Ok(());
        }
        Err(self.name_a_cycle(&indeg))
    }

    /// Walk the unsettled nodes until one repeats: the cycle, in dependency
    /// order, first node repeated only implicitly.
    fn name_a_cycle(&self, indeg: &HashMap<&Node, usize>) -> Vec<Node> {
        let stuck = |n: &Node| indeg.get(n).is_some_and(|d| *d > 0);
        let start = self
            .nodes
            .iter()
            .filter(|n| stuck(n))
            .min()
            .cloned()
            .expect("an unsettled graph has an unsettled node");
        let mut path: Vec<Node> = Vec::new();
        let mut cur = start;
        loop {
            if let Some(i) = path.iter().position(|n| *n == cur) {
                return path.split_off(i);
            }
            path.push(cur.clone());
            let next = self
                .needs(&cur)
                .filter(|e| e.demand == Demand::Content && stuck(&e.from))
                .map(|e| e.from.clone())
                .min();
            match next {
                Some(n) => cur = n,
                // Unreachable while `stuck` means "downstream of a cycle", but
                // a walk that cannot continue must still terminate.
                None => return path,
            }
        }
    }
}

/// The cycle as one sentence, for the load error.
pub fn describe_cycle(cycle: &[Node]) -> String {
    let mut names: Vec<String> = cycle.iter().map(|n| n.label()).collect();
    if let Some(first) = cycle.first() {
        names.push(first.label());
    }
    names.join(" → ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Key;

    fn out(u: &str) -> Node {
        Node::Output(Key::new(u))
    }
    fn inp(p: &str) -> Node {
        Node::Input(Key::new(p))
    }

    /// A facts cycle is legal, and this is the shape the corpus actually
    /// ships: a pool fold with no `where` selects its own route, so
    /// `/all.xml` is its own `route_members` member (io_folds.rs asserts the
    /// `<loc>` list that proves it). If the check looked at every edge instead
    /// of at content edges, every such site would fail to load.
    #[test]
    fn a_facts_cycle_is_legal_because_facts_are_complete_at_planning() {
        let g = Graph::from_edges([
            (out("/all.xml"), out("/all.xml"), Demand::Facts),
            (out("/a/"), out("/all.xml"), Demand::Facts),
            (inp("a.md"), out("/a/"), Demand::Content),
        ]);
        assert!(g.check_acyclic().is_ok());
        // And the pull terminates rather than chasing the self-edge: the fold
        // needs its member's facts, and nothing else.
        let order = g.pull(&out("/all.xml"));
        assert_eq!(order, vec![out("/a/"), out("/all.xml")]);
    }

    /// The tripwire, armed. The engine cannot describe this today — content
    /// edges run input → output, renditions included (I12, measured) — so the
    /// edge list is built by hand. That remains the honest place for it: a
    /// detector for a shape nothing can build is tested where the shape can be
    /// built, and manufacturing the shape in a live site would test the fixture
    /// rather than the engine.
    #[test]
    fn a_content_cycle_between_outputs_is_named() {
        let g = Graph::from_edges([
            (out("/a.png"), out("/b.png"), Demand::Content),
            (out("/b.png"), out("/a.png"), Demand::Content),
            (inp("a.png"), out("/a.png"), Demand::Content),
        ]);
        let cycle = g.check_acyclic().expect_err("the cycle is found");
        assert_eq!(cycle.len(), 2, "{cycle:?}");
        let said = describe_cycle(&cycle);
        assert!(said.contains("/a.png"), "{said}");
        assert!(said.contains("/b.png"), "{said}");
        // Named in dependency order and closed, so the sentence reads as a
        // loop rather than as a list.
        assert_eq!(said.matches("/a.png").count(), 2, "{said}");
    }

    /// One output deriving from its own bytes — the degenerate cycle Kahn
    /// cannot see, because a self-edge never raises anything's in-degree.
    #[test]
    fn a_content_self_edge_is_a_cycle_too() {
        let g = Graph::from_edges([(out("/x.png"), out("/x.png"), Demand::Content)]);
        let cycle = g.check_acyclic().expect_err("a self-edge is a cycle");
        assert_eq!(describe_cycle(&cycle), "output /x.png → output /x.png");
    }

    /// The pull's ordering claim, on a chain the engine cannot build either:
    /// dependencies before dependents, the target last.
    #[test]
    fn the_pull_puts_dependencies_first() {
        let g = Graph::from_edges([
            (inp("a.png"), out("/a.png"), Demand::Content),
            (out("/a.png"), out("/a-256.png"), Demand::Content),
            (out("/a-256.png"), out("/page/"), Demand::Content),
        ]);
        let order = g.pull(&out("/page/"));
        assert_eq!(
            order,
            vec![
                inp("a.png"),
                out("/a.png"),
                out("/a-256.png"),
                out("/page/"),
            ]
        );
    }
}

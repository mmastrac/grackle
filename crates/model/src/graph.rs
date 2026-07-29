//! Dependency graph over inputs and outputs.
//!
//! Built from `Route.inputs` (content) and `Route.route_members` (facts). Adds
//! no facts of its own.

pub use crate::{Demand, Edge, Node};

use crate::SiteDb;
use std::collections::{HashMap, HashSet};

/// Dependency graph of a planned site.
#[derive(Debug, Default)]
pub struct Graph {
    edges: Vec<Edge>,
    /// Incoming edges by dependent.
    into: HashMap<Node, Vec<usize>>,
    /// Outgoing edges by dependency.
    from: HashMap<Node, Vec<usize>>,
    nodes: HashSet<Node>,
}

impl Graph {
    /// Graph from a finished `SiteDb`.
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

    /// Graph from an explicit edge list (for tests).
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

    /// Edges this node needs.
    pub fn needs(&self, n: &Node) -> impl Iterator<Item = &Edge> {
        self.into
            .get(n)
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .map(|i| &self.edges[*i])
    }

    /// Edges that need this node.
    pub fn needed_by(&self, n: &Node) -> impl Iterator<Item = &Edge> {
        self.from
            .get(n)
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .map(|i| &self.edges[*i])
    }

    /// Outputs whose content may change if `n` changes (content edges only).
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

    /// Materialization order for one output: dependencies first, target last.
    ///
    /// Content edges recurse; facts edges are leaves (planning already has them).
    pub fn pull(&self, target: &Node) -> Vec<Node> {
        if !self.has(target) {
            return Vec::new();
        }
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
                    continue;
                }
                match e.demand {
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

    /// Error if any content edge forms a cycle. Facts cycles are allowed.
    pub fn check_acyclic(&self) -> Result<(), Vec<Node>> {
        // Fast path: content edges only leave inputs today.
        if !self
            .edges
            .iter()
            .any(|e| e.demand == Demand::Content && matches!(e.from, Node::Output(_)))
        {
            return Ok(());
        }
        if let Some(e) = self
            .edges
            .iter()
            .find(|e| e.demand == Demand::Content && e.from == e.to)
        {
            return Err(vec![e.from.clone()]);
        }
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
                None => return path,
            }
        }
    }
}

/// Cycle as a readable sentence for errors.
pub fn describe_cycle(cycle: &[Node]) -> String {
    let mut names: Vec<String> = cycle.iter().map(|n| n.label()).collect();
    if let Some(first) = cycle.first() {
        names.push(first.label());
    }
    names.join(" -> ")
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

    #[test]
    fn a_facts_cycle_is_legal_because_facts_are_complete_at_planning() {
        let g = Graph::from_edges([
            (out("/all.xml"), out("/all.xml"), Demand::Facts),
            (out("/a/"), out("/all.xml"), Demand::Facts),
            (inp("a.md"), out("/a/"), Demand::Content),
        ]);
        assert!(g.check_acyclic().is_ok());
        let order = g.pull(&out("/all.xml"));
        assert_eq!(order, vec![out("/a/"), out("/all.xml")]);
    }

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
        assert_eq!(said.matches("/a.png").count(), 2, "{said}");
    }

    #[test]
    fn a_content_self_edge_is_a_cycle_too() {
        let g = Graph::from_edges([(out("/x.png"), out("/x.png"), Demand::Content)]);
        let cycle = g.check_acyclic().expect_err("a self-edge is a cycle");
        assert_eq!(describe_cycle(&cycle), "output /x.png -> output /x.png");
    }

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

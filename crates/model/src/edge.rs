//! One dependency edge in the site graph.

use crate::{Demand, Node};

/// One edge, dependency → dependent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edge {
    pub from: Node,
    pub to: Node,
    pub demand: Demand,
}

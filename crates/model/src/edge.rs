//! One dependency edge.

use crate::{Demand, Node};

/// Edge from dependency to dependent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edge {
    pub from: Node,
    pub to: Node,
    pub demand: Demand,
}

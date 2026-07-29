//! A compiled neighbour query.

use crate::{Pool, RelLabel};
use grackle_db::filter;

/// Neighbour query over the two-row environment.
#[derive(Debug, Clone)]
pub struct Relation {
    pub name: String,
    /// Candidate pool (set, collection, or derived name).
    pub pool: Pool,
    /// Which `self` rows carry this relation; `None` means all.
    pub scope: Option<globset::GlobMatcher>,
    pub filter: filter::Filter,
    pub rank: Option<filter::Rank>,
    pub min_rank: Option<f64>,
    pub limit: usize,
    pub label: RelLabel,
}

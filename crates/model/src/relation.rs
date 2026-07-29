//! A compiled neighbour query (§6g).

use crate::{Pool, RelLabel};
use grackle_db::filter;

/// A compiled relation (§6g): a neighbour query over the two-row environment.
/// The expression ASTs are parsed and type-checked at load; the engine walks
/// candidates through `from → where → rank (+min_rank) → limit` per row.
#[derive(Debug, Clone)]
pub struct Relation {
    pub name: String,
    /// The candidate pool. A set/collection is row-independent; a derived
    /// name (`linked_from`) is row-relative — the difference the engine
    /// resolves per row.
    pub pool: Pool,
    /// Which `self` rows carry this relation (the `scope` glob), already
    /// compiled. `None` = every row of the collection.
    pub scope: Option<globset::GlobMatcher>,
    pub filter: filter::Filter,
    pub rank: Option<filter::Rank>,
    pub min_rank: Option<f64>,
    pub limit: usize,
    pub label: RelLabel,
}

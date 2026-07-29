//! Load-phase counts and timings.

use serde::Serialize;

#[derive(Debug, Default, Serialize)]
pub struct LoadStats {
    pub markers: usize,
    /// Sidecar `.toml` files found.
    pub sidecars: usize,
    pub markers_ms: f64,
    pub read_ms: f64,
    pub index_ms: f64,
    pub views_ms: f64,
}

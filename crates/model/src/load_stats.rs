//! Load-phase census and timings.

use serde::Serialize;

#[derive(Debug, Default, Serialize)]
pub struct LoadStats {
    pub markers: usize,
    /// Sidecar files found (IO.md I8) — a census beside the marker one, and
    /// for the same reason: a declaration family whose whole effect is on
    /// other files needs a count somebody can read.
    pub sidecars: usize,
    pub markers_ms: f64,
    pub read_ms: f64,
    pub index_ms: f64,
    pub views_ms: f64,
}

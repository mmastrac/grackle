//! One axis membership on a route.

use serde::Serialize;

/// Axis name, member value, and the row field that value sets while rendering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AxisMember {
    pub axis: String,
    pub value: String,
    pub field: String,
    /// First-declared member (`rel="canonical"`).
    pub canonical: bool,
}

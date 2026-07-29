//! Axis name a row's route rule spends.

use serde::Serialize;

/// Axis a row opted into via its route template.
#[derive(Debug, Clone, Serialize)]
pub struct RowAxis {
    pub name: String,
}

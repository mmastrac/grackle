//! Resolved rows for a routeless view.

use crate::Key;
use serde::Serialize;

/// Members of a named set or embed-only view.
#[derive(Debug, Default, Serialize)]
pub struct ViewRows {
    /// Layout when renderable; `None` for query-only sets.
    pub layout: Option<String>,
    /// Fragment variant for embeds.
    pub variant: Option<String>,
    pub rows: usize,
    #[serde(skip)]
    pub members: Vec<Key>,
}

//! A routeless view's resolved rows.

use crate::Key;
use serde::Serialize;

/// A routeless view's resolved rows.
#[derive(Debug, Default, Serialize)]
pub struct ViewRows {
    /// None means query-only: a named set, not something renderable.
    pub layout: Option<String>,
    /// Fragment variant (q24), for embedded rendering.
    pub variant: Option<String>,
    pub rows: usize,
    #[serde(skip)]
    pub members: Vec<Key>,
}

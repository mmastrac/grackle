//! One entry in a section or heading tree (§6e).

/// Source-shaped (parts come later so the same tree renders once per page
/// with only `current` moving).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineNode {
    pub label: String,
    /// None = an index-less directory: a label, deliberately unlinked
    /// (linking it would 404 — q27).
    pub url: Option<String>,
    pub order: Option<i64>,
    pub children: Vec<OutlineNode>,
}

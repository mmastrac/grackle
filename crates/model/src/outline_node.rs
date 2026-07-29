//! One node in a section or heading tree.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineNode {
    pub label: String,
    /// Absent for an index-less directory (label only).
    pub url: Option<String>,
    pub order: Option<i64>,
    pub children: Vec<OutlineNode>,
}

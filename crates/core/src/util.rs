//! Small shared helpers.

/// A comma-joined list, or `"(none)"` when empty — the shape the "known X"
/// diagnostics across the crate share.
pub(crate) fn join_or_none<S: AsRef<str>>(items: &[S]) -> String {
    if items.is_empty() {
        "(none)".to_string()
    } else {
        items
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

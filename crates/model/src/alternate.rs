//! One `rel="alternate"` head entry.

/// Alternate URL, with at most one of `hreflang` or `media_type`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Alternate {
    pub href: String,
    pub hreflang: Option<String>,
    pub media_type: Option<String>,
}

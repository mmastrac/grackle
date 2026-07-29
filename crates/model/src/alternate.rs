//! One `rel="alternate"` head entry.

/// An absolute URL, and at most one qualifier — `hreflang` for a translation,
/// `media_type` for a different-format form.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Alternate {
    pub href: String,
    pub hreflang: Option<String>,
    pub media_type: Option<String>,
}

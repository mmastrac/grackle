//! A citation's demand: source path plus rendition parameters.

use crate::Rendition;

/// One ask from a citation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ask {
    pub src: String,
    pub rendition: Rendition,
}

impl Ask {
    pub fn new(src: impl Into<String>, rendition: Rendition) -> Self {
        Self {
            src: src.into(),
            rendition,
        }
    }

    /// Default thumbnail ask for `src`.
    pub fn thumb(src: impl Into<String>) -> Self {
        Self::new(src, Rendition::THUMB)
    }
}

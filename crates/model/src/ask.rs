//! Demand: a source path and the rendition a citation asked for (IO.md §4a).

use crate::Rendition;

/// One demand: a source path as written in a citation, and the rendition it
/// asked for. Two asks for one image are two entries; one ask from two pages
/// is one.
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

    /// The engine's default thumb ask for `src`.
    pub fn thumb(src: impl Into<String>) -> Self {
        Self::new(src, Rendition::THUMB)
    }
}

//! Parsed `{% image %}` tag.

use crate::{Ask, Rendition};

/// Float mode, source, and rendition ask from one image tag.
pub struct ImageTag {
    pub mode: &'static str,
    pub src: String,
    pub rendition: Rendition,
}

impl ImageTag {
    /// Demand half used to key the rendition map.
    pub fn ask(&self) -> Ask {
        Ask::new(self.src.clone(), self.rendition)
    }
}

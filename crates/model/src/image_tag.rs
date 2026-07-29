//! `{% image %}` demand: float mode plus [`Ask`](crate::Ask) (IO.md §4a).

use crate::{Ask, Rendition};

/// What one `{% image %}` demands: a float mode, a source, and the rendition
/// parameters the citation asked for.
///
/// **This struct is IO.md §4a's "the citing edge carries the parameters"** —
/// the ask, as it is written. Parse lives in the engine so rendering and the
/// rendition pre-pass cannot disagree about what was asked for.
pub struct ImageTag {
    pub mode: &'static str,
    pub src: String,
    pub rendition: Rendition,
}

impl ImageTag {
    /// The demand half — what the rendition map keys on.
    pub fn ask(&self) -> Ask {
        Ask::new(self.src.clone(), self.rendition)
    }
}

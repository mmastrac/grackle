//! Transform parameters for a rendition output.
//!
//! Address is `hash(input bytes + variant())`. Parameters live on the output,
//! not the graph edge: every content edge into one rendition shares one ask.

use serde::{Deserialize, Serialize};

/// Fit box a citation asked a transform for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Rendition {
    /// Max width in pixels.
    pub max_w: u32,
    /// Max height, if constrained. `None` means width only.
    pub max_h: Option<u32>,
}

impl Rendition {
    /// Default thumbnail box. Spelling of [`variant`](Self::variant) is frozen
    /// (live published addresses).
    pub const THUMB: Rendition = Rendition {
        max_w: 640,
        max_h: Some(600),
    };

    /// Width-only fit.
    pub fn width(w: u32) -> Rendition {
        Rendition {
            max_w: w,
            max_h: None,
        }
    }

    /// Parameter string hashed with input bytes (and used as a cache key).
    pub fn variant(&self) -> String {
        match self.max_h {
            Some(h) => format!("fit{}x{h}-jpg85-pngbest-v1", self.max_w),
            None => format!("fitw{}-jpg85-pngbest-v1", self.max_w),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_variant_is_frozen() {
        assert_eq!(Rendition::THUMB.variant(), "fit640x600-jpg85-pngbest-v1");
    }

    #[test]
    fn different_asks_are_different_parameter_sets() {
        assert_ne!(
            Rendition::width(256).variant(),
            Rendition::width(512).variant()
        );
        assert_ne!(Rendition::width(640).variant(), Rendition::THUMB.variant());
    }
}

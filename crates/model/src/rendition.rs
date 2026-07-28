//! Renditions: the parameters a transform-bearing output was made with
//! (IO.md §4a, I12).
//!
//! A rendition — a resize, a re-encode — is **another output of the same
//! input**, and the thing that distinguishes it from every other output is a
//! parameter set. §4a says where that parameter set comes from: **demand.**
//! The citing edge asks (`{% image cover.png width=256 %}`), the pull
//! materializes exactly what citations requested, and an image's rendition set
//! is the union of its consumers' asks. There is no eager config surface —
//! srcset-style eager sets are a future opt-in on top of this, not the model.
//!
//! # Where the parameters live, and why not on the edge
//!
//! Review I-D asked the question this type answers: `graph::Edge` carries no
//! parameter slot, so demand-carried parameters need a home — extend the edge,
//! or a demands table keyed off it. **Neither: they live on the rendition
//! output**, and the hashing law is the argument.
//!
//! A rendition's address is `hash(input bytes + parameters)`. So every content
//! edge that arrives at one rendition output carries the *same* parameters, by
//! construction — two different asks are two different addresses and therefore
//! two different outputs, and the only way several inputs share one rendition
//! output is the untransformed-twin case (identical bytes, identical ask). A
//! parameter slot on the edge would therefore hold N copies of one value, with
//! nothing keeping the copies equal; a demands table keyed off the edge would
//! hold the same copies one indirection away. The output is where the value is
//! single, and `Graph` stays what I10 made it — a *view* of the join that adds
//! no facts of its own, every edge a key already sitting in a column.
//!
//! The citing edge still carries the ask in the only sense that matters: it
//! *names* the rendition, and the rendition carries the parameters. Following
//! the edge one step is how a pull gets from "this page wants a 256px cover"
//! to "run this transform on these bytes".
//!
//! # The variant string
//!
//! [`Rendition::variant`] is the spelling the hashing law hashes — the "plus
//! the transform parameters" half of `blake3(input bytes + parameters)`. The
//! default's spelling is **frozen**: it is the cache key and the published
//! address of every thumbnail the corpus ships, so changing it moves 260 URLs.
//! Everything else is new namespace.

use serde::{Deserialize, Serialize};

/// What a citation asked a transform for: a fit box.
///
/// The engine ships one transform (the thumbnail recipe: strip metadata, fit
/// within a box, shrink only, publish the smallest of {original, PNG, JPEG}),
/// so a rendition is that recipe plus its box. A second transform would add a
/// field here and a case to [`Rendition::variant`]; it would not change what a
/// rendition *is*.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Rendition {
    /// Fit within this many pixels wide.
    pub max_w: u32,
    /// …and this many tall, when the ask constrained height at all. `None` is
    /// a width-only ask, which is the shape `{% image … width=N %}` writes.
    pub max_h: Option<u32>,
}

impl Rendition {
    /// The engine's default rendition — the box `thumbnail.rb` used and every
    /// `{% image %}` without an explicit ask still gets.
    ///
    /// Frozen, together with its [`variant`](Rendition::variant) spelling:
    /// this pair is the address of every thumbnail grack.com publishes.
    pub const THUMB: Rendition = Rendition {
        max_w: 640,
        max_h: Some(600),
    };

    /// A width-only ask: fit within `w` pixels wide, height unconstrained.
    pub fn width(w: u32) -> Rendition {
        Rendition {
            max_w: w,
            max_h: None,
        }
    }

    /// The transform's parameter set as one string — what the hashing law
    /// hashes beside the input bytes, and what a cache entry is keyed by.
    ///
    /// **The `Some` arm's format is a published address**, not a debug
    /// spelling: `Rendition::THUMB.variant()` must stay
    /// `fit640x600-jpg85-pngbest-v1` for as long as those thumbnails are
    /// published at the addresses they have (`the_default_variant_is_frozen`
    /// is the pin). The trailing `-v1` is the recipe's own version: bump it to
    /// invalidate every cached rendition at once, which is a deliberate
    /// re-address rather than an accident.
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

    /// **The parity pin.** grack.com publishes 260 thumbnails at
    /// `/static/{blake3(bytes + this string)[..32]}.{ext}`, so this literal is
    /// a live address component. A test that recomputed it from the struct
    /// would assert nothing; the literal is the point.
    #[test]
    fn the_default_variant_is_frozen() {
        assert_eq!(Rendition::THUMB.variant(), "fit640x600-jpg85-pngbest-v1");
    }

    /// Two asks are two parameter sets are two addresses — which is what makes
    /// the demand union (§4a: "an image's rendition set is the union of its
    /// consumers' asks") a set rather than a collision.
    #[test]
    fn different_asks_are_different_parameter_sets() {
        assert_ne!(
            Rendition::width(256).variant(),
            Rendition::width(512).variant()
        );
        assert_ne!(Rendition::width(640).variant(), Rendition::THUMB.variant());
    }
}

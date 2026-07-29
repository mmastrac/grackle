//! Body markup without engine policy.
//!
//! Markdown AST work and the liquid-shaped tag *scan* live here. What a tag
//! *means* (views, thumbs, link resolution) is supplied by callbacks from the
//! engine — so this module never names `SiteDb`, `Config`, or a theme.

pub mod scan;

pub use grackle_model::Cite;

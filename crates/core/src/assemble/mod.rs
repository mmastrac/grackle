//! DOM assembly: part maps, binders, themes, and the render chain (THEME.md).
//!
//! Producers fill typed parts; fragments place them; `chain` decides which
//! rung of root → document → content a row enters at.

pub mod base;
pub mod binder;
pub mod chain;
pub mod parts;
pub mod slots;
pub mod theme;

pub use binder::Fragments;
pub use parts::{
    document, document_tree, fill_from_fields, member_face, page_row, preview, raw, require_face,
    row, tree_trail, Part, PartMap, PartType, Preview, Schemas,
};
pub use theme::{Theme, Themes};

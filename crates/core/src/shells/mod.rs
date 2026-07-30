//! Built-in fold shells and related site artifacts (IO.md §4, §5g).
//!
//! Map/fold vocabulary and the `renders` law live in `grackle_source::shell`.
//! This module holds the serializations those names select: atom, sitemap,
//! search, CSS, and registered script shells.

pub mod atom;
pub mod css;
pub mod script;
pub mod search;
pub mod sitemap;

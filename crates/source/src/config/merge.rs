//! Extends / rung merge (MERGE.md).
//!
//! The free functions live next to the `Shaped` impls in [`super::types`]
//! because those impls need private fields of `Config`. This module is the
//! documented home: call sites can `use crate::config::merge::*`.

#[allow(unused_imports)] // documented re-export surface; defs live in `types`
pub(crate) use super::types::{
    brace_alternatives, collection_key, describe_collection, engine_defaults, fence, identity,
    is_templated, law_of, merge_base, merge_base_traced, merge_table, merge_to_depth, note_table,
    project, split_profile, BASE, FORCE, NOT_PROJECTABLE, PROJECTABLE,
};

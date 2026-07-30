//! The site engine, as a library (`grackle-core`).
//!
//! The CLI lives in the `grackle` package (`crates/cli`) and calls the same
//! entry points the fixture harness uses. Modules are `pub` so tests can
//! reach them, not as a stability promise.
//!
//! Crate-root aliases: `crate::model` is the domain SiteDb/Row/Route layer
//! (`grackle-model`); `crate::filter` is the generic mini-DB (`grackle-db`).
//! Do not confuse the two — historically `model` was aliased `db`.

pub use grackle_db::{filter, template};
pub use grackle_model as model;
pub use grackle_source::{config, load, shell, store, views};

/// The engine workspace root (`grackle/`). The site corpus sits one level
/// above (`root = ".."` in `grackle.toml`). Tests that read the real corpus
/// anchor here rather than the CWD (Cargo sets CWD to this package).
#[cfg(test)]
pub(crate) fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub mod assemble;
pub mod build;
pub mod debug;
pub mod diff;
pub mod embed;
pub mod highlight;
pub mod links;
pub mod markdown;
pub mod markup;
pub mod outline;
pub mod passes;
pub mod pipeline;
pub mod relate;
pub mod render;
pub mod rewrite;
pub mod serve;
pub mod shells;
pub mod tags;

pub use assemble::{base, binder, parts, slots, theme};
pub mod thumbs;
pub mod trails;
pub mod urls;

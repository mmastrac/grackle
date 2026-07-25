//! The engine, as a library.
//!
//! `grackle` was a binary crate, which meant `render_site` was unreachable
//! from `tests/` and every behaviour test had to be a `#[cfg(test)]` module
//! beside the code it tested. That is fine for a unit test of one rule and
//! wrong for a test whose subject is *a site* — a directory of files in, a
//! directory of files out. This split exists so `tests/fixtures.rs` can call
//! the same entry point `main.rs` does.
//!
//! The alternative was shelling out to the built binary per fixture, which is
//! slower, and turns an `anyhow` error chain into stderr text to scrape.
//!
//! Nothing here is a stability promise: the modules are `pub` so the test
//! harness can reach them, not because anyone should depend on them.

// The crate-root aliases every module reaches for as `crate::db`,
// `crate::filter`, `crate::config`. They were `use` in `main.rs`; they are
// `pub use` here for the same reason everything else is.
pub use grackle_db::{filter, template};
pub use grackle_model as db;
pub use grackle_source::{config, store, views};

/// The workspace root — grack.com's own directory. The engine's manifest is
/// two levels down (`grackle/crates/grackle`), and the site it was written
/// for is one above that, which is what `root = ".."` in `grackle.toml`
/// means. For tests that read the real corpus: anchored to the manifest
/// rather than the CWD, which Cargo sets to this package's directory.
#[cfg(test)]
pub(crate) fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub mod base;
pub mod binder;
pub mod build;
pub mod debug;
pub mod diff;
pub mod embed;
pub mod highlight;
pub mod links;
pub mod markdown;
pub mod outline;
pub mod parts;
pub mod passes;
pub mod relate;
pub mod render;
pub mod rewrite;
pub mod serve;
pub mod slots;
pub mod tags;
pub mod theme;
pub mod thumbs;
pub mod trails;
pub mod urls;

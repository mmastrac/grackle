//! Config, filesystem, and load: turns a site directory into `grackle-db` rows.

pub mod config;
pub mod filename;
pub mod git;
pub mod load;
pub mod markers;
pub mod relations;
pub mod schema;
mod shape;
pub mod shell;
pub mod sidecar;
pub mod store;
pub mod strong;
pub mod views;

pub use load::load;

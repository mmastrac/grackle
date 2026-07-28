//! The source layer: config, the filesystem, and the load that turns the two
//! into a database.
//!
//! Sits above `grackle-db` and writes into it. The direction is the point:
//! the database has no idea where rows come from, and this is the half that
//! knows a site is a directory of files described by a `grackle.toml`.

pub mod config;
mod effective;
pub mod filename;
pub mod load;
pub mod markers;
pub mod relations;
pub mod schema;
mod shape;
pub mod shell;
pub mod sidecar;
pub mod store;
pub mod views;

pub use load::load;

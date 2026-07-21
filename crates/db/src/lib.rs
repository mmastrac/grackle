//! A mini database: tables of rows, a filter language, and the indexes over
//! them. Plus route templates, which are the one thing here that is about
//! URLs rather than storage.
//!
//! A row is whatever answers `filter::Row` — a field name in, a typed value
//! out. Everything else follows from that one contract, so nothing in this
//! crate knows what a grack.com row holds. That is `grackle-model`'s business,
//! and Cargo keeps it so: this crate depends on nothing in the workspace.

pub mod filter;
pub mod route;
pub mod table;

// The index shapes are `Table`'s to offer; only the collision report, which
// a caller has to render, is public.
mod index;

pub use filter::{Filter, Row, Schema, Type, Value};
pub use index::Collision;
pub use table::Table;
